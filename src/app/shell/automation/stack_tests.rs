//! Stacks through the control socket, as `rawmakase control stack` sends
//! them: photos grouped into stacks with the first on top, several stacks
//! one undoable step, and what is refused.
use super::{Hub, Settings, socket};
use crate::app::Editor;
use crate::app::testing::editor_with_catalog;
use crate::catalog::PhotoId;
use serde_json::{Value, json};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Sends `request` through the control socket announced in `data`, running
/// the editor's control requests until it is answered.
fn ask(e: &mut Editor, data: &std::path::Path, request: Value) -> Value {
    let connection = rawmakase_ctl::Connection::read(data).unwrap();
    let reply = std::thread::spawn(move || connection.request(request));
    let ctx = e.context.clone();
    let until = Instant::now() + Duration::from_secs(10);
    while !reply.is_finished() {
        e.control_commands(&ctx);
        assert!(Instant::now() < until, "no reply");
        std::thread::sleep(Duration::from_millis(2));
    }
    reply.join().unwrap().unwrap()
}

/// An editor whose catalog has five photos in one folder and two in
/// another, with a control socket announced in a data folder of its own.
/// Returns the first folder's photos, then the other's.
fn editor() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Editor,
    Vec<PhotoId>,
    Vec<PhotoId>,
) {
    let (dir, mut e, mut ids) =
        editor_with_catalog(&["a.RAF", "b.RAF", "c.RAF", "d.RAF", "e.RAF"]).unwrap();
    ids.sort();
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    for name in ["f.RAF", "g.RAF"] {
        std::fs::write(other.join(name), b"fixture").unwrap();
    }
    let catalog = dir.path().join("test.rawmakase");
    crate::catalog::Catalog::open(&catalog)
        .unwrap()
        .add_folder(&other)
        .unwrap();
    let library = crate::app::library::Library::load(&catalog, e.context.clone()).unwrap();
    let mut elsewhere: Vec<PhotoId> = library
        .session
        .photos()
        .iter()
        .map(|p| p.id)
        .filter(|p| !ids.contains(p))
        .collect();
    elsewhere.sort();
    e.library = Some(Box::new(library));
    let data = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::sync_channel(256);
    e.controls = Hub::new(Settings::default(), rx);
    e.controls.socket = Some(
        socket::start_at(
            tx,
            e.context.clone(),
            rawmakase_protocol::Endpoint::path(data.path()),
        )
        .unwrap(),
    );
    (dir, data, e, ids, elsewhere)
}

/// Each photo's place in its stack, (index, count), or none. A stack that
/// isn't collapsed fails the test: the command makes collapsed stacks.
fn places(e: &Editor, ids: &[PhotoId]) -> Vec<Option<(usize, usize)>> {
    let photos = e.library.as_ref().unwrap().session.photos();
    ids.iter()
        .map(|id| {
            photos
                .iter()
                .find(|p| p.id == *id)
                .and_then(|p| p.stack)
                .map(|s| {
                    assert!(s.collapsed, "a stack made by command is collapsed");
                    (s.index, s.count)
                })
        })
        .collect()
}

#[test]
fn photos_are_grouped_into_stacks_by_command_as_one_step() {
    let (_dir, data, mut e, ids, elsewhere) = editor();
    let id = |p: PhotoId| p.0;
    let files: Vec<_> = e
        .library
        .as_ref()
        .unwrap()
        .session
        .photos()
        .iter()
        .map(|p| (p.path.clone(), std::fs::read(&p.path).unwrap()))
        .collect();
    assert_eq!(elsewhere.len(), 2);

    // Two stacks at once, each with the photo named first on top.
    let reply = ask(
        &mut e,
        data.path(),
        json!({"cmd":"stack","stacks":[[id(ids[1]), id(ids[0])], [id(ids[2]), id(ids[3]), id(ids[4])]]}),
    );
    assert_eq!(
        reply["result"]["stacked"],
        json!({"stacks": 2, "photos": 5}),
        "{reply}"
    );
    assert_eq!(
        places(&e, &ids),
        [
            Some((1, 2)),
            Some((0, 2)),
            Some((0, 3)),
            Some((1, 3)),
            Some((2, 3))
        ]
    );

    // One step for Undo, however many stacks it made.
    e.sync_undo();
    e.undo();
    assert_eq!(places(&e, &ids), [None; 5]);
    e.redo();
    assert_eq!(places(&e, &ids)[1], Some((0, 2)));

    // The files are as they were.
    for (path, bytes) in files {
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn a_stack_of_one_across_folders_or_of_a_photo_named_twice_or_unknown_is_refused() {
    let (_dir, data, mut e, ids, elsewhere) = editor();
    let id = |p: PhotoId| p.0;
    for (request, code, says) in [
        (
            json!({"cmd":"stack","stacks":[[id(ids[0])]]}),
            "invalid_request",
            "two photos or more",
        ),
        (
            json!({"cmd":"stack","stacks":[[id(ids[0]), id(ids[1])], [id(ids[1]), id(ids[2])]]}),
            "invalid_request",
            "named twice",
        ),
        (
            json!({"cmd":"stack","stacks":[[id(ids[0]), id(elsewhere[0])]]}),
            "invalid_request",
            "different folders",
        ),
        // One stack that can't be made refuses the others with it.
        (
            json!({"cmd":"stack","stacks":[[id(ids[0]), id(ids[1])], [id(ids[2]), id(elsewhere[0])]]}),
            "invalid_request",
            "different folders",
        ),
        (
            json!({"cmd":"stack","stacks":[[id(ids[0]), 9999]]}),
            "not_found",
            "9999",
        ),
    ] {
        let reply = ask(&mut e, data.path(), request.clone());
        assert_eq!(reply["code"], code, "{request}: {reply}");
        assert!(
            reply["error"].as_str().unwrap().contains(says),
            "{request}: {reply}"
        );
    }
    assert_eq!(places(&e, &elsewhere), [None; 2]);
    assert_eq!(places(&e, &ids), [None; 5]);
}
