use super::*;
use fancy_regex::{Captures, Regex};
use std::sync::LazyLock;

pub static RE_VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{(\w+)\}\}").unwrap());
pub fn interpolate_variables(text: &mut String) {
    *text = RE_VARIABLE
        .replace_all(text, |caps: &Captures<'_>| {
            let key = &caps[1];
            match key {
                "__os__" => env::consts::OS.to_string(),
                "__os_distro__" => {
                    let info = os_info::get();
                    if env::consts::OS == "linux" {
                        format!("{info} (linux)")
                    } else {
                        info.to_string()
                    }
                }
                "__os_family__" => env::consts::FAMILY.to_string(),
                "__arch__" => env::consts::ARCH.to_string(),
                "__shell__" => SHELL.name.clone(),
                "__locale__" => sys_locale::get_locale().unwrap_or_default(),
                "__now__" => now(),
                "__cwd__" => env::current_dir()
                    .map(|v| v.display().to_string())
                    .unwrap_or_default(),
                "__researcher_search_instructions__" => {
                    if std::env::var("AICHAT_WSLINKS")
                        .map(|v| v == "true" || v == "1")
                        .unwrap_or(false)
                    {
                        "1. Search for information on the given topic (use web_search with links=true to discover source URLs)\n2. Fetch 2-4 relevant pages for detail using fetch_and_summarize (no more)\n3. Return a concise, structured summary of your findings".to_string()
                    } else {
                        "1. Search for information on the given topic using web_search (with links=false). The tool returns a grounded, comprehensive summary with source citations directly.\n2. Return a concise, structured summary of your findings based on the grounded search results. Do NOT fetch individual web pages.".to_string()
                    }
                }
                _ => format!("{{{{{key}}}}}"),
            }
        })
        .to_string();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_researcher_search_instructions_interpolation() {
        let mut text_default = "{{__researcher_search_instructions__}}".to_string();
        std::env::remove_var("AICHAT_WSLINKS");
        interpolate_variables(&mut text_default);
        assert!(text_default.contains("with links=false"));
        assert!(text_default.contains("Do NOT fetch individual web pages"));

        let mut text_links = "{{__researcher_search_instructions__}}".to_string();
        std::env::set_var("AICHAT_WSLINKS", "true");
        interpolate_variables(&mut text_links);
        assert!(text_links.contains("links=true"));
        assert!(text_links.contains("fetch_and_summarize"));
        std::env::remove_var("AICHAT_WSLINKS");
    }
}
