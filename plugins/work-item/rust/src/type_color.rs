//! How a work item type looks: a tone of the app's palette for the common ones, so a dot sits with the other dots.

/// The tone of a work item type's dot; a type we don't know is muted.
pub fn type_tone(kind: &str) -> &'static str {
    let kind = kind.to_lowercase();
    let has = |names: &[&str]| names.iter().any(|name| kind.contains(name));
    if has(&["bug"]) {
        "danger"
    } else if kind == "task" {
        "warn"
    } else if has(&["user story", "product backlog item", "requirement"]) {
        "info"
    } else if has(&["feature"]) {
        "success"
    } else {
        "muted"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_types_get_a_palette_tone_each_and_the_rest_muted() {
        assert_eq!(type_tone("Bug"), "danger");
        assert_eq!(type_tone("User Story"), "info");
        assert_eq!(type_tone("Product Backlog Item"), "info");
        assert_eq!(type_tone("Task"), "warn");
        assert_eq!(type_tone("Feature"), "success");
        assert_eq!(type_tone("Epic"), "muted");
        assert_eq!(type_tone("Whatever"), "muted");
    }
}
