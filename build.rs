use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
struct LanguagesFile {
    languages: HashMap<String, LanguageDef>,
}

#[derive(Deserialize)]
struct LanguageDef {
    #[serde(default)]
    line_comment: Vec<String>,
    #[serde(default)]
    multi_line_comments: Vec<Vec<String>>,
    #[serde(default)]
    extensions: Vec<String>,
}

fn main() {
    let languages_json_path =
        env::var("MARGINALIA_LANGUAGES").expect("MARGINALIA_LANGUAGES env var must be set");
    println!("cargo:rerun-if-env-changed=MARGINALIA_LANGUAGES");

    let json_str = fs::read_to_string(&languages_json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", languages_json_path, e));
    let file: LanguagesFile = serde_json::from_str(&json_str).expect("failed to parse languages.json");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("comment_tokens.rs");

    let mut code = String::new();
    code.push_str(
        "\
/// Comment syntax tokens for a language.
#[derive(Debug, Clone)]
pub struct CommentTokens {
    pub line: &'static [&'static str],
    pub block: &'static [(&'static str, &'static str)],
}

/// Look up comment tokens by file extension (without the leading dot).
pub fn comment_tokens(extension: &str) -> Option<CommentTokens> {
    match extension {
",
    );

    // Build a map from extension -> (line_comments, block_comments)
    type ExtInfo = (Vec<String>, Vec<(String, String)>);
    let mut ext_map: HashMap<String, ExtInfo> = HashMap::new();

    for lang in file.languages.values() {
        let blocks: Vec<(String, String)> = lang
            .multi_line_comments
            .iter()
            .filter_map(|pair| {
                if pair.len() == 2 {
                    Some((pair[0].clone(), pair[1].clone()))
                } else {
                    None
                }
            })
            .collect();

        for ext in &lang.extensions {
            ext_map
                .entry(ext.clone())
                .or_insert_with(|| (lang.line_comment.clone(), blocks.clone()));
        }
    }

    // Markdown files use HTML comments but tokei doesn't list them
    for ext in &["md", "markdown"] {
        ext_map
            .entry(ext.to_string())
            .and_modify(|e| {
                if e.0.is_empty() && e.1.is_empty() {
                    e.1 = vec![("<!--".to_string(), "-->".to_string())];
                }
            });
    }

    let mut extensions: Vec<_> = ext_map.keys().cloned().collect();
    extensions.sort();

    for ext in &extensions {
        let (line_comments, block_comments) = &ext_map[ext];

        let line_arr = line_comments
            .iter()
            .map(|c| format!("\"{}\"", c.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(", ");

        let block_arr = block_comments
            .iter()
            .map(|(open, close)| {
                format!(
                    "(\"{}\", \"{}\")",
                    open.replace('\\', "\\\\").replace('"', "\\\""),
                    close.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");

        code.push_str(&format!(
            "        \"{}\" => Some(CommentTokens {{ line: &[{}], block: &[{}] }}),\n",
            ext, line_arr, block_arr,
        ));
    }

    code.push_str(
        "\
        _ => None,
    }
}
",
    );

    fs::write(dest_path, code).expect("failed to write comment_tokens.rs");
}
