//! Helpers the tests of several modules share.

use std::path::Path;

use crate::{
    manifest::{Contributes, Manifest, Permissions, Plugin},
    post::PostRequest,
};

pub(crate) fn plugin(dir: &Path, exec: &[&str], transcript: &[&str]) -> Plugin {
    Plugin {
        manifest: Manifest {
            id: "t".into(),
            name: "T".into(),
            version: "0".into(),
            description: String::new(),
            api: 1,
            min_wings_version: None,
            main: "main.js".into(),
            icon: None,
            permissions: Permissions {
                exec: exec.iter().map(|s| s.to_string()).collect(),
                transcript: transcript.iter().map(|s| s.to_string()).collect(),
                open_url: vec!["https://github.com/".into()],
                fetch: vec!["https://api.example.com/v1/".into()],
                ..Default::default()
            },
            contributes: Contributes::default(),
        },
        dir: dir.to_path_buf(),
        icon_svg: None,
    }
}

pub(crate) fn post_request(program: &str, url: &str, host: Option<&str>) -> PostRequest {
    let fields = serde_json::json!({ "body": "Thanks, fixed" }).as_object().unwrap().clone();
    PostRequest { program: program.into(), url: url.into(), host: host.map(String::from), fields }
}

pub(crate) fn with_panes(panes: Option<&[&str]>) -> Plugin {
    let mut p = plugin(Path::new("/tmp"), &[], &[]);
    p.manifest.permissions.panes = panes.map(|list| list.iter().map(|s| s.to_string()).collect());
    p
}
