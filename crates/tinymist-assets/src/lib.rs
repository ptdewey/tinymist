/// If this file is not found, please refer to <https://enter-tainer.github.io/typst-preview/dev.html> to build the frontend.
#[cfg(feature = "typst-preview")]
pub const TYPST_PREVIEW_HTML: &str = include_str!("typst-preview.html");
#[cfg(not(feature = "typst-preview"))]
pub const TYPST_PREVIEW_HTML: &str = "<html><body>Typst Preview needs to be built with the `embed-html` feature to work!</body></html>";

/// If this file is not found, please runs `yarn extract:l10n:rs` to extract the
/// localization data.
#[cfg(feature = "l10n")]
pub const L10N_DATA: &str = include_str!("tinymist-rt.toml");
#[cfg(not(feature = "l10n"))]
pub const L10N_DATA: &str = "";
