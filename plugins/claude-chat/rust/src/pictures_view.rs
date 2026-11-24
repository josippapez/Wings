//! Pictures in the conversation: thumbnails in your messages and under the tool call that gave them back, and the dialog
//! that shows one large when you click it. Every picture shows; only one Wings wouldn't keep is a note.

use wings_plugin::ui::{Node, dialog, image, muted, row};

use crate::{
    chat::{self, Chat},
    images,
};

/// A thumbnail's box, in points, and the box of the picture in the dialog, which the app also keeps within the window.
const THUMB: (u32, u32) = (200, 150);
const LARGE: u32 = 1_800;
const DIALOG_WIDTH: u32 = 960;

/// The pictures `ids` as a wrapping row of thumbnails, each opening the dialog. `what` is how a screen reader names them.
pub fn thumbs(ids: &[String], what: &str) -> Option<Node> {
    if ids.is_empty() {
        return None;
    }
    let shown = ids.iter().enumerate().map(|(at, id)| {
        let name = if ids.len() > 1 { format!("{what} {} of {}", at + 1, ids.len()) } else { what.to_owned() };
        if !images::is_shown(id) {
            return muted("Picture not kept").prop("size", "small").key(format!("pic-{at}"));
        }
        let open = id.clone();
        image(id, &name, THUMB.0, THUMB.1).prop("radius", 8).on("onClick", move |_| chat::show_picture(&open)).key(format!("pic-{at}"))
    });
    Some(row().prop("gap", "s").prop("wrap", true).children(shown))
}

/// The picture you opened, large, over the chat. Escape and a click outside close it.
pub fn dialog_of(c: &Chat) -> Option<Node> {
    let id = c.picture.as_ref()?;
    let large = image(id, "Picture, enlarged", LARGE, LARGE).prop("fitWindow", true);
    Some(dialog("Picture", DIALOG_WIDTH, chat::close_picture).child(row().prop("justify", "center").child(large)))
}
