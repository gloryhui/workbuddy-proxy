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
        } else if path.contains("chevron-right") {
            r#"<path d="m9 6 6 6-6 6"/>"#
        } else if path.contains("chevron") {
            r#"<path d="m6 9 6 6 6-6"/>"#
        } else if path.contains("search") {
            r#"<circle cx="10" cy="10" r="6"/><path d="m15 15 6 6"/>"#
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
