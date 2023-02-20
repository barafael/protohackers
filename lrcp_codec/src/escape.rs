use crate::ESCAPE;

pub fn escape(string: &str) -> String {
    let mut token = String::new();
    let chars = string.chars();
    for ch in chars {
        if ch == ESCAPE || ch == '/' {
            token.push(ESCAPE);
        }
        token.push(ch);
    }
    token
}
