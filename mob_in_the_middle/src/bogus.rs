use lazy_static::lazy_static;
use regex::Regex;

pub const TONYS_ADDRESS: &str = "7YWHMfk9JZe0LM0g1ZauHuiSxhI";

lazy_static! {
    pub static ref RE: Regex = Regex::new(r"(7\w{25,34})( |$)").unwrap();
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn examples() {
        assert!(RE.is_match("7F1u3wSD5RbOHQmupo9nx4TnhQ"));
        assert!(RE.is_match("7iKDZEwPZSqIvDnHvVN2r0hUWXD5rHX"));
        assert!(RE.is_match("7LOrwbDlS8NujgjddyogWgIM93MV5N2VR"));
        assert!(RE.is_match("7adNeSwJkMakpEcln9HEtthSRtxdmEHOT8T"));
    }

    #[test]
    fn replaces() {
        let text = " please pay bgcoin 540 to 7adNeSwJkMakpEcln9HEtthSRtxdmEHOT8T now.";
        let result = RE.replace(text, format!("{TONYS_ADDRESS}$2"));
        assert_eq!(
            result,
            " please pay bgcoin 540 to 7YWHMfk9JZe0LM0g1ZauHuiSxhI now."
        )
    }

    #[test]
    fn replaces_2() {
        let text = "[BigFrank692] Send refunds to 78LQ1UzAp7GAvWuTLzKFntxfO0 please.";
        let result = RE.replace(text, format!("{TONYS_ADDRESS}$2"));
        assert_eq!(
            result,
            "[BigFrank692] Send refunds to 7YWHMfk9JZe0LM0g1ZauHuiSxhI please."
        );
    }

    #[test]
    fn too_long() {
        let text = "[RichWizard12] This is too long: 7QDPU8nAmrWK5UbGDPt8LVKnCZSG3OLznPsc";
        let result = RE.replace(text, format!("{TONYS_ADDRESS}$2"));
        assert_eq!(result, text);
    }
}
