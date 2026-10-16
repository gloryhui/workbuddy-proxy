use gpui_kit::{AssetSource, SharedString};
use std::borrow::Cow;
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "brand.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../../assets/icon-128.png"
            ))));
        }
        if !path.ends_with(".svg") {
            return Ok(None);
        }
        let shape = if path.contains("eye-off") {
            r#"<path d="m3 3 18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M9.9 5.2A11 11 0 0 1 12 5c5 0 9 7 9 7a15 15 0 0 1-2.5 3.2M6.5 6.5C4.3 8.2 3 12 3 12s4 7 9 7a10 10 0 0 0 5.5-1.6"/>"#
        } else if path.contains("eye") {
            r#"<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/>"#
        } else if path.contains("check") {
            r#"<path d="m5 12 4 4L19 6"/>"#
        } else if path.contains("chevron-up") {
            r#"<path d="m6 15 6-6 6 6"/>"#
        } else if path.contains("chevron-left") {
            r#"<path d="m15 6-6 6 6 6"/>"#
        } else if path.contains("chevron-right") {
            r#"<path d="m9 6 6 6-6 6"/>"#
        } else if path.contains("chevron") {
            r#"<path d="m6 9 6 6 6-6"/>"#
        } else if path.contains("search") {
            r#"<circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/>"#
        } else if path.contains("clock") {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"#
        } else if path.contains("calendar") {
            r#"<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M16 3v4M8 3v4M3 11h18M8 15h.01M12 15h.01M16 15h.01M8 18h.01M12 18h.01"/>"#
        } else if path.contains("autostart") {
            r#"<path d="M12 2v10M18.36 5.64a9 9 0 1 1-12.73 0"/>"#
        } else if path.contains("globe") {
            r#"<circle cx="12" cy="12" r="10"/><ellipse cx="12" cy="12" rx="4" ry="10"/><path d="M2 12h20"/>"#
        } else if path.contains("github") {
            r#"<path d="M9 19c-4.3 1.4-4.3-2.5-6-3m12 6v-3.87a3.37 3.37 0 0 0-.94-2.61c3.14-.35 6.44-1.54 6.44-7A5.44 5.44 0 0 0 19 4.77a5.07 5.07 0 0 0-.09-3.77S17.73.65 15 2.48a13.38 13.38 0 0 0-7 0C5.27.65 4.09 1 4.09 1A5.07 5.07 0 0 0 4 4.77a5.44 5.44 0 0 0-1.5 3.78c0 5.42 3.3 6.61 6.44 7A3.37 3.37 0 0 0 8 18.13V22"/>"#
        } else if path.contains("feedback") {
            r#"<path d="M2 21a8 8 0 0 1 10.821-7.487M21.378 16.626a1 1 0 0 0-3.004-3.004l-4.01 4.012a2 2 0 0 0-.506.854l-.837 2.87a.5.5 0 0 0 .62.62l2.87-.837a2 2 0 0 0 .854-.506Z"/><circle cx="10" cy="8" r="5"/>"#
        } else if path.contains("restore-size") {
            r#"<path d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M21 16v3a2 2 0 0 1-2 2h-3M8 21H5a2 2 0 0 1-2-2v-3"/>"#
        } else if path.contains("window-minimize") {
            r#"<path d="M5 12h14"/>"#
        } else if path.contains("window-maximize") {
            r#"<rect x="5" y="5" width="14" height="14" rx="1"/>"#
        } else if path.contains("window-restore") {
            r#"<path d="M9 5h10v10"/><rect x="5" y="9" width="10" height="10" rx="1"/>"#
        } else if path.contains("window-close") {
            r#"<path d="m6 6 12 12M6 18 18 6"/>"#
        } else if path.contains("inbox") {
            r#"<path d="m4 4-2 12v4h20v-4L20 4Z M2 16h6l2 3h4l2-3h6"/>"#
        } else if path.contains("arrow-up") {
            r#"<path d="m6 12 6-6 6 6M12 6v12"/>"#
        } else if path.contains("arrow-down") {
            r#"<path d="m6 12 6 6 6-6M12 6v12"/>"#
        } else if path.contains("copy") {
            r#"<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1"/>"#
        } else if path.contains("model-edit") {
            r#"<path d="M12 4H5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-7M16 3a2.1 2.1 0 0 1 3 3L10 15l-4 1 1-4Z"/>"#
        } else if path.contains("delete") {
            r#"<path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/>"#
        } else if path.contains("refresh") {
            r#"<path d="M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 1l1 5M4 12l1 5a8 8 0 0 0 13 1"/>"#
        } else if path.contains("minus") {
            r#"<path d="M5 12h14"/>"#
        } else if path.contains("close") || path.contains("/x.") {
            r#"<path d="m6 6 12 12M6 18 18 6"/>"#
        } else {
            r#"<path d="M5 12h14M12 5v14"/>"#
        };
        Ok(Some(Cow::Owned(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">{shape}</svg>"#).into_bytes())))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
