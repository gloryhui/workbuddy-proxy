//! Process-local Studio fonts. The original upstream faces are embedded once;
//! no system font installation or filesystem access is needed while painting.
use gpui_kit::component::Theme;
use gpui_kit::{App, FontFallbacks, Global, SharedString, Styled, TextStyleRefinement};
use std::borrow::Cow;

const SANS_FACES: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Medium.otf"),
    include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"),
];
const LATIN_FACES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
];

struct StudioFonts {
    original_sans: SharedString,
    attempted: bool,
    sans: Option<SharedString>,
    latin: Option<SharedString>,
    latin_fallbacks: Option<FontFallbacks>,
}

impl Global for StudioFonts {}

/// Keep old skins' native font, including after a Studio -> old-skin switch.
/// Loading is lazy: a session that only uses an old skin never opens these faces.
/// Reuse the first registration's family inventory for the immediately following
/// theme installation; later skin changes still query the current inventory.
pub(crate) fn prepare(studio: bool, cx: &mut App) -> Option<Vec<String>> {
    if !cx.has_global::<StudioFonts>() {
        let original_sans = Theme::global(cx).font_family.clone();
        cx.set_global(StudioFonts {
            original_sans,
            attempted: false,
            sans: None,
            latin: None,
            latin_fallbacks: None,
        });
    }
    if !studio || cx.global::<StudioFonts>().attempted {
        return None;
    }
    cx.global_mut::<StudioFonts>().attempted = true;

    // Static faces are deliberate. The pinned Apple text system matches
    // loaded faces by weight; it does not select a variable font's wght axis.
    let sans_ok = cx
        .text_system()
        .add_fonts(SANS_FACES.iter().map(|face| Cow::Borrowed(*face)).collect())
        .is_ok();
    // GPUI's Apple memory source resolves primary faces, but its CoreText
    // cascade resolves fallback families by name. Make just our CJK faces
    // discoverable within this process as well, without writing font files.
    #[cfg(target_os = "macos")]
    let cascade_ok = sans_ok && SANS_FACES.iter().all(|face| macos::register(face));
    #[cfg(not(target_os = "macos"))]
    let cascade_ok = sans_ok;
    let latin_ok = cx
        .text_system()
        .add_fonts(
            LATIN_FACES
                .iter()
                .map(|face| Cow::Borrowed(*face))
                .collect(),
        )
        .is_ok()
        && cascade_ok;
    let names = cx.text_system().all_font_names();
    let find = |family: &str| {
        names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(family))
            .cloned()
            .map(SharedString::from)
    };
    let sans = sans_ok.then(|| find("Noto Sans SC")).flatten();
    let latin = (latin_ok && sans.is_some())
        .then(|| find("Inter"))
        .flatten();
    if sans.is_none() || latin.is_none() {
        // Bounded startup diagnostic only; a failed family keeps native text.
        eprintln!("Studio font registration incomplete; unavailable families use native fallback");
    }
    let fallbacks = sans
        .as_ref()
        .map(|name| FontFallbacks::from_fonts(vec![name.to_string()]));
    let state = cx.global_mut::<StudioFonts>();
    state.sans = sans;
    state.latin = latin;
    state.latin_fallbacks = fallbacks;
    Some(names)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{ffi::c_void, ptr};

    type Ref = *const c_void;
    type ReleaseData = Option<unsafe extern "C" fn(*mut c_void, Ref, usize)>;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGDataProviderCreateWithData(
            info: *mut c_void,
            data: Ref,
            size: usize,
            release: ReleaseData,
        ) -> Ref;
        fn CGDataProviderRelease(provider: Ref);
        fn CGFontCreateWithDataProvider(provider: Ref) -> Ref;
        fn CGFontRelease(font: Ref);
    }
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterGraphicsFont(font: Ref, error: *mut Ref) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: Ref);
        fn CFErrorGetCode(error: Ref) -> isize;
    }

    pub(super) fn register(bytes: &'static [u8]) -> bool {
        // SAFETY: the provider borrows immutable embedded bytes for the whole
        // process lifetime. CoreText retains the registered graphics font;
        // every Create-owned local reference and returned error is released.
        // This memory-font API is process-local. Its newer descriptor API
        // rejects these memory-only descriptors (303 in the native QA probe),
        // so a URL-based replacement would require unnecessary disk copies.
        unsafe {
            let provider = CGDataProviderCreateWithData(
                ptr::null_mut(),
                bytes.as_ptr().cast(),
                bytes.len(),
                None,
            );
            if provider.is_null() {
                return false;
            }
            let font = CGFontCreateWithDataProvider(provider);
            if font.is_null() {
                CGDataProviderRelease(provider);
                return false;
            }
            let mut error = ptr::null();
            let registered = CTFontManagerRegisterGraphicsFont(font, &mut error);
            // A pre-existing face with this PostScript name already supplies
            // name matching; do not unregister or replace the user's font.
            let already_registered = !error.is_null() && CFErrorGetCode(error) == 105;
            if !error.is_null() {
                CFRelease(error);
            }
            CGFontRelease(font);
            CGDataProviderRelease(provider);
            registered || already_registered
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::{CStr, CString, c_char};

        unsafe extern "C" {
            fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32)
            -> Ref;
            fn CFStringGetCString(
                string: Ref,
                buffer: *mut c_char,
                size: isize,
                encoding: u32,
            ) -> bool;
            fn CTFontCreateWithName(name: Ref, size: f64, matrix: Ref) -> Ref;
            fn CTFontCopyPostScriptName(font: Ref) -> Ref;
            fn CTFontGetGlyphsForCharacters(
                font: Ref,
                chars: *const u16,
                glyphs: *mut u16,
                count: isize,
            ) -> bool;
        }

        #[test]
        fn embedded_cjk_faces_resolve_by_name_after_registration_and_duplicate_registration() {
            const UTF8: u32 = 0x0800_0100;
            for (bytes, name) in super::super::SANS_FACES.iter().zip([
                "NotoSansSC-Regular",
                "NotoSansSC-Medium",
                "NotoSansSC-Bold",
            ]) {
                assert!(register(bytes));
                assert!(register(bytes), "already-registered is a successful no-op");
                let expected = CString::new(name).unwrap();
                // SAFETY: all native references are Create/Copy-owned; each
                // is released after CoreText resolves its face and glyph.
                unsafe {
                    let key = CFStringCreateWithCString(ptr::null(), expected.as_ptr(), UTF8);
                    assert!(!key.is_null());
                    let font = CTFontCreateWithName(key, 15., ptr::null());
                    assert!(!font.is_null());
                    let actual = CTFontCopyPostScriptName(font);
                    let mut buffer = [0 as c_char; 128];
                    assert!(CFStringGetCString(
                        actual,
                        buffer.as_mut_ptr(),
                        buffer.len() as isize,
                        UTF8
                    ));
                    assert_eq!(CStr::from_ptr(buffer.as_ptr()), expected.as_c_str());
                    let mut glyph = 0;
                    assert!(CTFontGetGlyphsForCharacters(font, &0x4e07, &mut glyph, 1));
                    assert_ne!(glyph, 0, "localized numeric unit 万 must resolve");
                    CFRelease(actual);
                    CFRelease(font);
                    CFRelease(key);
                }
            }
        }
    }
}

pub(crate) fn original_sans(cx: &App) -> Option<SharedString> {
    cx.try_global::<StudioFonts>()
        .map(|fonts| fonts.original_sans.clone())
}

pub(crate) fn sans(cx: &App) -> Option<SharedString> {
    cx.try_global::<StudioFonts>()
        .and_then(|fonts| fonts.sans.clone())
}

/// Use Inter only for the Latin/number roles identified in the Figma frames.
/// Noto remains the explicit fallback for localized units such as 万 and 亿.
pub(crate) fn latin<T: Styled>(mut text: T, cx: &App) -> T {
    if let Some(style) = latin_style(cx) {
        text.text_style().font_family = style.font_family;
        text.text_style().font_fallbacks = style.font_fallbacks;
    }
    text
}

/// The native plot painter scopes only its date labels to this same role.
pub(crate) fn latin_style(cx: &App) -> Option<TextStyleRefinement> {
    if crate::theme::current_skin() != crate::theme::Skin::StudioIndigo {
        return None;
    }
    let fonts = cx.try_global::<StudioFonts>()?;
    Some(TextStyleRefinement {
        font_family: Some(fonts.latin.clone()?),
        font_fallbacks: fonts.latin_fallbacks.clone(),
        ..Default::default()
    })
}
