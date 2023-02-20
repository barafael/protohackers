use crate::ESCAPE;

pub fn unescape(string: &str) -> String {
    let mut token = String::new();
    let mut chars = string.chars();
    while let Some(ch) = chars.next() {
        match ch {
            ESCAPE => {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            _ => token.push(ch),
        }
    }
    token
}
