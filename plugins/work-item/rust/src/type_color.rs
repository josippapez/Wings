//! How a work item type looks: Azure DevOps' own colours for the common ones.

/// The colour of a work item type's dot; a type we don't know is grey.
pub fn type_color(kind: &str) -> u32 {
    let kind = kind.to_lowercase();
    let has = |names: &[&str]| names.iter().any(|name| kind.contains(name));
    if has(&["bug"]) {
        0xcc293d
    } else if kind == "task" {
        0xf2cb1d
    } else if has(&["user story", "product backlog item", "requirement"]) {
        0x009ccc
    } else if has(&["feature"]) {
        0x773b93
    } else if has(&["epic"]) {
        0xff7b00
    } else if has(&["issue", "impediment", "risk"]) {
        0xb4009e
    } else if has(&["test"]) {
        0x004b50
    } else {
        0x8a8a8a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azures_colours_go_to_the_common_types_and_grey_to_the_rest() {
        assert_eq!(type_color("Bug"), 0xcc293d);
        assert_eq!(type_color("User Story"), 0x009ccc);
        assert_eq!(type_color("Product Backlog Item"), 0x009ccc);
        assert_eq!(type_color("Task"), 0xf2cb1d);
        assert_eq!(type_color("Test Case"), 0x004b50);
        assert_eq!(type_color("Epic"), 0xff7b00);
        assert_eq!(type_color("Whatever"), 0x8a8a8a);
    }
}
