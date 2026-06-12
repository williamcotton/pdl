use std::fs;
use std::path::Path;

const LANGUAGE_FILE: &str = "PDL_LANG.md";
const LANGUAGE_REFERENCE: &str = include_str!("../templates/PDL_LANG.md");
const MARKER: &str = "<!-- pdl-init-language-reference -->";

pub(crate) fn init_agent_files(
    dir: &Path,
    codex: bool,
    claude: bool,
    agy: bool,
) -> Result<Vec<String>, String> {
    if !codex && !claude && !agy {
        return Err("choose at least one agent target: --codex, --claude, or --agy".to_string());
    }
    if dir.exists() && !dir.is_dir() {
        return Err(format!("`{}` is not a directory", dir.display()));
    }
    fs::create_dir_all(dir)
        .map_err(|error| format!("could not create `{}`: {error}", dir.display()))?;

    let mut actions = Vec::new();
    actions.push(ensure_exact_file(
        &dir.join(LANGUAGE_FILE),
        LANGUAGE_REFERENCE,
    )?);

    if codex || agy {
        actions.push(ensure_agent_reference(
            &dir.join("AGENTS.md"),
            "Agent Instructions",
        )?);
    }
    if claude {
        actions.push(ensure_agent_reference(
            &dir.join("CLAUDE.md"),
            "Claude Instructions",
        )?);
    }

    Ok(actions)
}

fn ensure_exact_file(path: &Path, content: &str) -> Result<String, String> {
    if path.exists() {
        let existing = fs::read_to_string(path)
            .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
        if existing == content {
            Ok(format!("unchanged {}", path.display()))
        } else {
            Err(format!(
                "refusing to overwrite existing `{}`; move it aside or merge it manually",
                path.display()
            ))
        }
    } else {
        fs::write(path, content)
            .map_err(|error| format!("could not write `{}`: {error}", path.display()))?;
        Ok(format!("wrote {}", path.display()))
    }
}

fn ensure_agent_reference(path: &Path, title: &str) -> Result<String, String> {
    let block = reference_block();
    if path.exists() {
        let mut existing = fs::read_to_string(path)
            .map_err(|error| format!("could not read `{}`: {error}", path.display()))?;
        if existing.contains(MARKER) || existing.contains(LANGUAGE_FILE) {
            return Ok(format!("unchanged {}", path.display()));
        }
        if !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(block);
        fs::write(path, existing)
            .map_err(|error| format!("could not write `{}`: {error}", path.display()))?;
        Ok(format!("updated {}", path.display()))
    } else {
        let content = format!("# {title}\n{block}");
        fs::write(path, content)
            .map_err(|error| format!("could not write `{}`: {error}", path.display()))?;
        Ok(format!("wrote {}", path.display()))
    }
}

fn reference_block() -> &'static str {
    "\n<!-- pdl-init-language-reference -->\n## PDL Language Reference\n\nThis project uses PDL. Before creating or editing `.pdl` files, read `PDL_LANG.md` at the project root. Use `pdl check file.pdl` for diagnostics and `pdl fmt file.pdl --check` before handing code back.\n<!-- /pdl-init-language-reference -->\n"
}
