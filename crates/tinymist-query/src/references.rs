use std::sync::OnceLock;

use tinymist_analysis::adt::interner::Interned;
use tinymist_std::typst::TypstDocument;
use typst::syntax::Span;

use crate::{
    analysis::{Definition, SearchCtx},
    prelude::*,
    syntax::{get_index_info, RefExpr, SyntaxClass},
    StrRef,
};

/// The [`textDocument/references`] request is sent from the client to the
/// server to resolve project-wide references for the symbol denoted by the
/// given text document position.
///
/// [`textDocument/references`]: https://microsoft.github.io/language-server-protocol/specification#textDocument_references
#[derive(Debug, Clone)]
pub struct ReferencesRequest {
    /// The path of the document to request for.
    pub path: PathBuf,
    /// The source code position to request for.
    pub position: LspPosition,
}

impl StatefulRequest for ReferencesRequest {
    type Response = Vec<LspLocation>;

    fn request(self, ctx: &mut LocalContext, graph: LspComputeGraph) -> Option<Self::Response> {
        let doc = graph.snap.success_doc.as_ref();
        let source = ctx.source_by_path(&self.path).ok()?;
        let syntax = ctx.classify_for_decl(&source, self.position)?;

        let locations = find_references(ctx, &source, doc, syntax)?;

        crate::log_debug_ct!("references: {locations:?}");
        Some(locations)
    }
}

pub(crate) fn find_references(
    ctx: &mut LocalContext,
    source: &Source,
    doc: Option<&TypstDocument>,
    syntax: SyntaxClass<'_>,
) -> Option<Vec<LspLocation>> {
    let finding_label = match syntax {
        SyntaxClass::VarAccess(..) | SyntaxClass::Callee(..) => false,
        SyntaxClass::Label { .. } | SyntaxClass::Ref(..) => true,
        SyntaxClass::ImportPath(..) | SyntaxClass::IncludePath(..) | SyntaxClass::Normal(..) => {
            return None;
        }
    };

    let def = ctx.def_of_syntax(source, doc, syntax)?;

    let worker = ReferencesWorker {
        ctx: ctx.fork_for_search(),
        references: vec![],
        def,
        module_path: OnceLock::new(),
    };

    if finding_label {
        worker.label_root()
    } else {
        // todo: reference of builtin items?
        worker.ident_root()
    }
}

struct ReferencesWorker<'a> {
    ctx: SearchCtx<'a>,
    references: Vec<LspLocation>,
    def: Definition,
    module_path: OnceLock<StrRef>,
}

impl ReferencesWorker<'_> {
    fn label_root(mut self) -> Option<Vec<LspLocation>> {
        for ref_fid in self.ctx.ctx.depended_files() {
            self.file(ref_fid)?;
        }

        Some(self.references)
    }

    fn ident_root(mut self) -> Option<Vec<LspLocation>> {
        self.file(self.def.decl.file_id()?);
        while let Some(ref_fid) = self.ctx.worklist.pop() {
            self.file(ref_fid);
        }

        Some(self.references)
    }

    fn file(&mut self, ref_fid: TypstFileId) -> Option<()> {
        log::debug!("references: file: {ref_fid:?}");
        let src = self.ctx.ctx.source_by_id(ref_fid).ok()?;
        let index = get_index_info(&src);
        match self.def.decl.kind() {
            DefKind::Constant | DefKind::Function | DefKind::Struct | DefKind::Variable => {
                if !index.identifiers.contains(self.def.decl.name()) {
                    return Some(());
                }
            }
            DefKind::Module => {
                let ref_by_ident = index.identifiers.contains(self.def.decl.name());
                let ref_by_path = index.paths.contains(self.module_path());
                if !(ref_by_ident || ref_by_path) {
                    return Some(());
                }
            }
            DefKind::Reference => {}
        }

        let ei = self.ctx.ctx.expr_stage(&src);
        let uri = self.ctx.ctx.uri_for_id(ref_fid).ok()?;

        let t = ei.get_refs(self.def.decl.clone());
        self.push_idents(&ei.source, &uri, t);

        if ei.is_exported(&self.def.decl) {
            self.ctx.push_dependents(ref_fid);
        }

        Some(())
    }

    fn push_idents<'b>(
        &mut self,
        src: &Source,
        url: &Url,
        idents: impl Iterator<Item = (&'b Span, &'b Interned<RefExpr>)>,
    ) {
        self.push_ranges(src, url, idents.map(|(span, _)| span));
    }

    fn push_ranges<'b>(&mut self, src: &Source, url: &Url, spans: impl Iterator<Item = &'b Span>) {
        self.references.extend(spans.filter_map(|span| {
            // todo: this is not necessary a name span
            let range = self.ctx.ctx.to_lsp_range(src.range(*span)?, src);
            Some(LspLocation {
                uri: url.clone(),
                range,
            })
        }));
    }

    // todo: references of package
    fn module_path(&self) -> &StrRef {
        self.module_path.get_or_init(|| {
            self.def
                .decl
                .file_id()
                .and_then(|fid| {
                    fid.vpath()
                        .as_rooted_path()
                        .file_name()?
                        .to_str()
                        .map(From::from)
                })
                .unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::find_module_level_docs;
    use crate::tests::*;

    #[test]
    fn test() {
        snapshot_testing("references", &|ctx, path| {
            let source = ctx.source_by_path(&path).unwrap();

            let docs = find_module_level_docs(&source).unwrap_or_default();
            let properties = get_test_properties(&docs);
            let doc = compile_doc_for_test(ctx, &properties);

            let request = ReferencesRequest {
                path: path.clone(),
                position: find_test_position(&source),
            };

            let result = request.request(ctx, doc);
            let mut result = result.map(|v| {
                v.into_iter()
                    .map(|loc| {
                        let fp = file_path(loc.uri.as_str());
                        format!(
                            "{fp}@{}:{}:{}:{}",
                            loc.range.start.line,
                            loc.range.start.character,
                            loc.range.end.line,
                            loc.range.end.character
                        )
                    })
                    .collect::<Vec<_>>()
            });
            // sort
            if let Some(result) = result.as_mut() {
                result.sort();
            }

            assert_snapshot!(JsonRepr::new_pure(result));
        });
    }
}
