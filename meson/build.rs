//! Generate Valence models from `schemas/`, then patch trait `*Model` DateTime
//! fields to use `valence::datetime_unix` (upstream trait codegen omits it).

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn patch_trait_model_datetime_unix(src: &str) -> String {
    // Insert serde(with) on required DateTime fields inside `pub struct *Model`.
    // Matches FileModel.uploaded_at and any future trait models with the same shape.
    let mut out = String::with_capacity(src.len() + 128);
    let mut in_trait_model = false;
    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("pub struct ") && trimmed.ends_with("Model {") {
            in_trait_model = true;
        } else if in_trait_model && trimmed == "}" {
            in_trait_model = false;
        }

        if in_trait_model
            && trimmed.ends_with(": chrono::DateTime<chrono::Utc>,")
            && !trimmed.starts_with("//")
        {
            // Avoid double-patching if already present on previous line.
            if !out.ends_with("valence::datetime_unix\")]\n")
                && !out.ends_with("valence::datetime_unix\")]\r\n")
            {
                let indent = line
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect::<String>();
                let _ = writeln!(out, "{indent}#[serde(with = \"valence::datetime_unix\")]");
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let schemas_dir = PathBuf::from("schemas");

    println!("cargo:rerun-if-changed=schemas/");

    valence_codegen::generate_models(&valence_codegen::CodegenConfig {
        schemas_dir,
        out_dir: out_dir.clone(),
        file_suffix: "_valence_schema.rs",
        trait_file_suffix: "_valence_trait.rs",
    })?;

    let generated = out_dir.join("generated_models.rs");
    let raw = fs::read_to_string(&generated)?;
    let patched = patch_trait_model_datetime_unix(&raw);
    if patched != raw {
        fs::write(&generated, patched)?;
    }
    Ok(())
}
