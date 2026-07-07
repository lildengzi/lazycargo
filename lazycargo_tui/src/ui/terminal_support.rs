use std::io;

use arboard::Clipboard;

pub(super) fn first_url(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
}

pub(super) fn open_url(url: &str) -> io::Result<()> {
    open::that(url).map_err(io::Error::other)
}

pub(super) fn copy_to_clipboard(text: &str) -> io::Result<()> {
    Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
        .map_err(io::Error::other)
}
