use std::collections::HashMap;

// Minimal {{var}} interpolator. No escaping, simple and predictable for mock/testing.
pub fn render_template(template: &str, vars: &HashMap<String, String>) -> String {
    // Simple scan & replace
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    let bytes = template.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            // find closing }}
            if let Some(end) = find_close(template, i + 2) {
                let key = template[i + 2..end].trim();
                if let Some(val) = vars.get(key) {
                    out.push_str(val);
                } else {
                    // keep original text if not found
                    out.push_str(&template[i..end + 2]);
                }
                i = end + 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_close(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}
