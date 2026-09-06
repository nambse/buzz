/// Validate the private native/frontend build contract without runtime overrides.
/// Shared by build.rs and the bounded compiled-policy test harness.
pub fn validate_private_build_flags(
    native: Option<&str>,
    frontend: Option<&str>,
    slug: Option<&str>,
) -> Result<bool, &'static str> {
    match native {
        Some("1") if frontend == Some("true") && slug == Some("ortak-private-20260905") => Ok(true),
        None if frontend != Some("true") && slug != Some("ortak-private-20260905") => Ok(false),
        _ => Err("Private Ortak requires ORTAK_PRIVATE_DESKTOP=1, VITE_ORTAK_PRIVATE_MODE=true and the selected BUZZ_BUILD_DEMO_SLUG together"),
    }
}

#[cfg(test)]
mod private_build_flag_tests {
    use super::validate_private_build_flags as validate;

    #[test]
    fn mismatched_private_builds_fail_instead_of_shipping_a_frontend_only_guard() {
        let slug = Some("ortak-private-20260905");
        assert_eq!(validate(Some("1"), Some("true"), slug), Ok(true));
        for values in [
            (None, Some("true"), slug),
            (Some("0"), Some("true"), slug),
            (Some("1"), None, slug),
            (Some("1"), Some("false"), slug),
            (Some("1"), Some("true"), None),
            (None, None, slug),
        ] {
            assert!(validate(values.0, values.1, values.2).is_err());
        }
        assert_eq!(validate(None, None, None), Ok(false));
        assert_eq!(validate(None, Some("false"), Some("other-demo")), Ok(false));
    }
}
