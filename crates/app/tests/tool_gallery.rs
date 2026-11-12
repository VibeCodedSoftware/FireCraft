//! A visual check of every drawing and paint tool: one gesture per tool on
//! a fresh document, rendered to a PNG for a human (or a reviewer) to look
//! at. Ignored by default; run with
//! `FIRECRAFT_GALLERY=out.png cargo test -p app --test tool_gallery -- --ignored --nocapture`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use app::editor::{AutoShape, Editor, Tool};

const CELL_W: f64 = 128.0;
const CELL_H: f64 = 120.0;

fn cell(index: usize) -> (f64, f64) {
    let col = (index % 5) as f64;
    let row = (index / 5) as f64;
    (col * CELL_W, row * CELL_H)
}

fn drag(editor: &mut Editor, tool: Tool, index: usize) -> String {
    let (x, y) = cell(index);
    editor.tool = tool;
    editor.begin_drag(geom::Point::new(x + 20.0, y + 20.0));
    editor.drag_to(geom::Point::new(x + 70.0, y + 60.0));
    editor.drag_to(geom::Point::new(x + CELL_W - 20.0, y + CELL_H - 20.0));
    match editor.end_drag() {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("ERR {error}"),
    }
}

fn stroke(editor: &mut Editor, tool: Tool, index: usize) -> String {
    let (x, y) = cell(index);
    editor.tool = tool;
    let points = [(20.0, 30.0), (50.0, 80.0), (80.0, 40.0), (108.0, 90.0)];
    let start = geom::Point::new(x + points[0].0, y + points[0].1);
    if let Err(error) = editor.begin_stroke(start) {
        return format!("ERR begin {error}");
    }
    for (px, py) in &points[1..] {
        if let Err(error) = editor.stroke_to(geom::Point::new(x + px, y + py)) {
            return format!("ERR stroke {error}");
        }
    }
    match editor.end_stroke() {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("ERR end {error}"),
    }
}

#[test]
#[ignore = "timing probe; run in release with --nocapture"]
fn painting_cost_per_pointer_move() {
    for tool in [Tool::Paintbrush, Tool::Pencil, Tool::Eraser] {
        let mut editor = Editor::untitled();
        editor.brush.size = 16;
        // A painted canvas first, so the eraser has pixels to remove.
        editor.tool = Tool::Paintbrush;
        editor.begin_stroke(geom::Point::new(20.0, 240.0)).unwrap();
        editor.stroke_to(geom::Point::new(620.0, 240.0)).unwrap();
        editor.end_stroke().unwrap();
        editor.tool = tool;
        let start = std::time::Instant::now();
        editor.begin_stroke(geom::Point::new(20.0, 240.0)).unwrap();
        for step in 1..=200 {
            let x = 20.0 + f64::from(step) * 3.0;
            let y = 240.0 + (f64::from(step) / 10.0).sin() * 40.0;
            editor.stroke_to(geom::Point::new(x, y)).unwrap();
        }
        editor.end_stroke().unwrap();
        let per_move = start.elapsed().as_secs_f64() * 1000.0 / 200.0;
        let render = std::time::Instant::now();
        for _ in 0..10 {
            compose::render(editor.document(), 0, 0).unwrap();
        }
        let per_render = render.elapsed().as_secs_f64() * 1000.0 / 10.0;
        println!("{tool:?}: {per_move:.2} ms per move, {per_render:.2} ms per page render");
    }
}

#[test]
#[ignore = "writes a PNG for visual review"]
fn every_tool_draws_something_visible() {
    let mut editor = Editor::untitled();
    editor.brush.color = [220, 40, 40, 255];
    editor.brush.size = 8;
    let mut index = 0;
    for tool in [Tool::Rect, Tool::Ellipse, Tool::Line] {
        println!("{tool:?}: {}", drag(&mut editor, tool, index));
        index += 1;
    }
    for shape in AutoShape::ALL {
        println!("{}: {}", shape.label(), drag(&mut editor, Tool::AutoShape(shape), index));
        index += 1;
    }
    for tool in [Tool::Paintbrush, Tool::Pencil] {
        println!("{tool:?}: {}", stroke(&mut editor, tool, index));
        index += 1;
    }
    // The bucket floods inside a closed pencil loop on its own cell: the
    // loop's inside turns red, the rest of the bitmap stays clear.
    let (x, y) = cell(index);
    editor.tool = Tool::Pencil;
    let corners = [(20.0, 20.0), (108.0, 20.0), (108.0, 100.0), (20.0, 100.0), (20.0, 20.0)];
    editor.begin_stroke(geom::Point::new(x + corners[0].0, y + corners[0].1)).unwrap();
    for (px, py) in &corners[1..] {
        editor.stroke_to(geom::Point::new(x + px, y + py)).unwrap();
    }
    editor.end_stroke().unwrap();
    editor.tool = Tool::PaintBucket;
    editor.brush.color = [40, 120, 220, 255];
    println!(
        "PaintBucket: {:?}",
        editor.fill_target(geom::Point::new(x + 60.0, y + 60.0))
    );

    let render = compose::render(editor.document(), 0, 0).expect("page renders");
    // The render is premultiplied; over white that is straight enough
    // for a visual check once the alpha is composited.
    let mut rgba = render.pixels.clone();
    for px in rgba.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        for c in 0..3 {
            px[c] = (u16::from(px[c]) + (255 - a)).min(255) as u8;
        }
        px[3] = 255;
    }
    let plane = io::Plane {
        width: u32::from(render.width),
        height: u32::from(render.height),
        rgba: &rgba,
    };
    let png = io::png::encode(&plane).expect("png encodes");
    let out = std::env::var("FIRECRAFT_GALLERY").unwrap_or_else(|_| "tool_gallery.png".to_owned());
    std::fs::write(&out, png).expect("png written");
    println!("wrote {out}");
}
