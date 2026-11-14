//! Rendering fixtures include mesh preparation, separate from editor CPU layout.
use super::*;
use egui::epaint::{text::Galley, Mesh, TessellationOptions, Tessellator, TextShape};
fn render(galley: Arc<Galley>, offset: f32) -> Mesh {
    let mut tess = Tessellator::new(
        galley.pixels_per_point,
        TessellationOptions::default(),
        [2048, 2048],
        vec![],
    );
    tess.set_clip_rect(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(1200.0, 600.0),
    ));
    let shape = TextShape::new(egui::pos2(-offset, 0.0), galley, egui::Color32::WHITE);
    let mut out = Mesh::default();
    tess.tessellate_text(&shape, &mut out);
    out
}
#[test]
#[ignore = "manual optimized CPU/mesh allocation profile of viewport text rendering"]
fn profile_viewport_text_rendering() {
    for (name, source) in [
        ("1M-line", "abcdefghij".repeat(100_000)),
        (
            "40k-lines",
            "    let value = compute(item);\n".repeat(40_000),
        ),
    ] {
        let galley = row_index::tests::layout(&source, f32::INFINITY, 1.0);
        for fraction in [0.0, 0.5, 1.0] {
            let offset = (galley.rect.width() - 1000.0).max(0.0) * fraction;
            let first = Instant::now();
            let out = render(galley.clone(), offset);
            let cold = first.elapsed().as_secs_f64() * 1000.0;
            let bytes = out.vertices.len() * std::mem::size_of::<egui::epaint::Vertex>()
                + out.indices.len() * 4;
            let reserved = out.vertices.capacity() * std::mem::size_of::<egui::epaint::Vertex>()
                + out.indices.capacity() * 4;
            drop(out);
            let start = Instant::now();
            for _ in 0..10 {
                std::hint::black_box(render(galley.clone(), offset));
            }
            eprintln!("Render {name} x={fraction}: cold={cold:.3}ms warm={:.3}ms vertices={} submitted={}B reserved={}B", start.elapsed().as_secs_f64()*100.0, render(galley.clone(),offset).vertices.len(), bytes, reserved);
        }
    }
}
#[test]
fn a_long_unwrapped_row_submits_only_viewport_meshes() {
    let galley = row_index::tests::layout(&"abcdefghij".repeat(10_000), f32::INFINITY, 1.0);
    for fraction in [0.0, 0.5, 1.0] {
        let out = render(galley.clone(), (galley.rect.width() - 1000.0) * fraction);
        assert!(
            out.vertices.len() < 10_000,
            "offscreen glyphs must not be submitted"
        );
        assert!(
            out.vertices.capacity() < 20_000,
            "offscreen glyphs must not be reserved"
        );
    }
}

fn mesh(shape: &TextShape, clip: egui::Rect, culling: bool, round: bool) -> Mesh {
    let options = TessellationOptions {
        coarse_tessellation_culling: culling,
        round_text_to_pixels: round,
        ..Default::default()
    };
    let mut tess = Tessellator::new(shape.galley.pixels_per_point, options, [2048, 2048], vec![]);
    tess.set_clip_rect(clip);
    let mut out = Mesh::default();
    tess.tessellate_text(shape, &mut out);
    assert!(out.is_valid());
    out
}
fn visible_triangles(mesh: &Mesh, clip: egui::Rect) -> Vec<[egui::epaint::Vertex; 3]> {
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|ids| {
            let tri = [
                mesh.vertices[ids[0] as usize],
                mesh.vertices[ids[1] as usize],
                mesh.vertices[ids[2] as usize],
            ];
            let bounds = tri.iter().fold(egui::Rect::NOTHING, |r, v| {
                r.union(egui::Rect::from_min_max(v.pos, v.pos))
            });
            clip.intersects(bounds).then_some(tri)
        })
        .collect()
}
fn equivalent(shape: &TextShape, clip: egui::Rect, round: bool) {
    let reference = mesh(shape, clip, false, round);
    let actual = mesh(shape, clip, true, round);
    assert_eq!(
        visible_triangles(&actual, clip),
        visible_triangles(&reference, clip),
        "all potentially visible triangle attributes and paint order must match"
    );
}
fn styled(text: &str, scale: f32) -> (egui::Context, Arc<Galley>) {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(scale);
    let mut result = None;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            for (n, line) in text.split_inclusive('\n').enumerate() {
                job.append(
                    line,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::monospace(14.0 + (n % 3) as f32),
                        color: if n % 2 == 0 {
                            egui::Color32::PLACEHOLDER
                        } else {
                            egui::Color32::LIGHT_BLUE
                        },
                        background: egui::Color32::from_rgb(35, 40, 45),
                        underline: egui::Stroke::new(1.0_f32, egui::Color32::GREEN),
                        strikethrough: egui::Stroke::new(0.7_f32, egui::Color32::RED),
                        italics: true,
                        ..Default::default()
                    },
                );
            }
            result = Some(ui.fonts(|f| f.layout_job(job)));
        });
    });
    (ctx, result.unwrap())
}
#[test]
fn viewport_meshes_preserve_visible_triangles_across_styles_scales_edges_and_rotation() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let (_ctx, g) = styled(
            &format!(
                "{}\n{}",
                "αe\u{301}🦀 italic text ".repeat(100),
                "second ".repeat(300)
            ),
            scale,
        );
        for fraction in [0.0, 0.25, 0.5, 1.0] {
            for round in [false, true] {
                let mut shape = TextShape::new(
                    egui::pos2(0.375 - g.rect.width() * fraction, 0.625),
                    g.clone(),
                    egui::Color32::WHITE,
                );
                shape.opacity_factor = 0.6;
                let clip =
                    egui::Rect::from_min_max(egui::pos2(0.25, 0.25), egui::pos2(320.5, 80.5));
                equivalent(&shape, clip, round);
                shape.override_text_color = Some(egui::Color32::YELLOW);
                equivalent(&shape, clip, round);
                shape.angle = 0.35;
                equivalent(&shape, clip, round);
            }
        }
    }
}
#[test]
fn warm_mesh_indexes_handle_selection_vertex_reordering_and_sparse_ranges() {
    let (_ctx, g) = styled(&"selection background ".repeat(1000), 1.25);
    let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 40.0));
    let mut shape = TextShape::new(
        egui::pos2(-g.rect.width() / 2.0, 0.0),
        g,
        egui::Color32::WHITE,
    );
    equivalent(&shape, clip, true);
    assert!(shape.galley.rows[0].visuals.mesh_cache.bytes() > 0);
    let (a, b) = (
        shape.galley.from_ccursor(egui::text::CCursor::new(0)),
        shape.galley.end(),
    );
    egui::text_selection::visuals::paint_text_selection(
        &mut shape.galley,
        &egui::Visuals::dark(),
        &egui::text_selection::CursorRange {
            primary: b,
            secondary: a,
        },
        None,
    );
    assert_eq!(shape.galley.rows[0].visuals.mesh_cache.bytes(), 0);
    equivalent(&shape, clip, true);
    let out = mesh(&shape, clip, true, true);
    assert!(
        out.vertices.len() < 10_000,
        "scattered background indices must not copy the entire row"
    );
}
#[test]
fn translating_keeps_shared_meshes_scaling_invalidates_and_serialization_omits_metadata() {
    let g = row_index::tests::layout(&"abcdefghij".repeat(2000), f32::INFINITY, 1.0);
    let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 40.0));
    let shape = TextShape::new(
        egui::pos2(-g.rect.width() / 2.0, 0.0),
        g.clone(),
        egui::Color32::WHITE,
    );
    equivalent(&shape, clip, true);
    assert!(g.rows[0].visuals.mesh_cache.bytes() > 0);
    let mut moved = egui::Shape::Text(shape);
    moved.translate(egui::vec2(30.5, 2.25));
    if let egui::Shape::Text(shape) = &moved {
        assert!(
            Arc::ptr_eq(&shape.galley, &g),
            "translation must not clone a long glyph mesh"
        );
        equivalent(shape, clip, true);
    } else {
        unreachable!();
    }
    moved.scale(1.5);
    if let egui::Shape::Text(shape) = &moved {
        assert_eq!(shape.galley.rows[0].visuals.mesh_cache.bytes(), 0);
        equivalent(shape, clip, true);
        let json = serde_json::to_string(&shape.galley.rows[0].visuals).unwrap();
        assert!(!json.contains("mesh_cache"));
        let restored: egui::epaint::text::RowVisuals = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.mesh_cache.bytes(), 0);
        assert_eq!(restored, shape.galley.rows[0].visuals);
    } else {
        unreachable!();
    }
}
#[test]
fn rtl_reordering_invalidates_warm_mesh_bounds_without_dropping_visible_triangles() {
    let (ctx, g) = styled(&"لا مرحبا שלום English ".repeat(100), 1.25);
    let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 40.0));
    let shape = TextShape::new(
        egui::pos2(-g.rect.width() / 2.0, 0.0),
        g.clone(),
        egui::Color32::WHITE,
    );
    equivalent(&shape, clip, true);
    assert!(g.rows[0].visuals.mesh_cache.bytes() > 0);
    let mut result = None;
    let _ = ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            result = Some(ui.fonts(|f| bidi_view::apply(f, g.clone(), bidi_view::Base::Auto).0));
        });
    });
    let g = result.unwrap();
    assert_eq!(g.rows[0].visuals.mesh_cache.bytes(), 0);
    for fraction in [0.0, 0.5, 1.0] {
        equivalent(
            &TextShape::new(
                egui::pos2(-g.rect.width() * fraction, 0.0),
                g.clone(),
                egui::Color32::WHITE,
            ),
            clip,
            true,
        );
    }
}
#[test]
fn multiline_reservation_is_limited_to_visible_rows_and_cache_initialization_is_shared() {
    let source = "short line of source text\n".repeat(5000);
    let g = row_index::tests::layout(&source, f32::INFINITY, 1.0);
    let out = render(g, 0.0);
    assert!(
        out.vertices.capacity() < 20_000,
        "reserve only visible row meshes"
    );
    let g = row_index::tests::layout(&"abcdefghij".repeat(3000), f32::INFINITY, 1.0);
    let cold = (*g).clone();
    assert_eq!(g.rows[0].visuals.mesh_cache.bytes(), 0);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let g = g.clone();
            thread::spawn(move || {
                render(g.clone(), 0.0);
                g.rows[0].visuals.mesh_cache.bytes()
            })
        })
        .collect();
    let sizes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(sizes.iter().all(|&size| size > 0 && size == sizes[0]));
    assert_eq!(*g, cold, "derived bounds must not change galley equality");
}

#[test]
fn moving_reused_layout_blocks_invalidates_warm_mesh_bounds() {
    let ctx = egui::Context::default();
    let mut cache = editor_layout::LayoutCache::default();
    let source = format!("{}\n", "abcdefghij".repeat(200)).repeat(20);
    let mut layout = |text: &str| {
        let mut g = None;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                g = Some(cache.layout(
                    ui,
                    text,
                    hedit::syntax::Language::PlainText,
                    editor_layout::Font {
                        size: 14.0,
                        line_height: None,
                    },
                    f32::INFINITY,
                    &hedit::theme::Theme::default(),
                ));
            });
        });
        g.unwrap()
    };
    let first = layout(&source);
    let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, first.rect.height()));
    mesh(
        &TextShape::new(
            egui::pos2(-first.rect.width() / 2.0, 0.0),
            first.clone(),
            egui::Color32::WHITE,
        ),
        clip,
        true,
        true,
    );
    assert!(first.rows[10].visuals.mesh_cache.bytes() > 0);
    let next = layout(&format!("prefix\n{source}"));
    assert_eq!(next.rows[11].text(), first.rows[10].text());
    assert_eq!(next.rows[11].visuals.mesh_cache.bytes(), 0);
    let y = next.rows[11].rect.bottom() - 2.0;
    equivalent(
        &TextShape::new(
            egui::pos2(-next.rect.width() / 2.0, -y),
            next,
            egui::Color32::WHITE,
        ),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 20.0)),
        true,
    );
}
#[test]
fn extreme_horizontal_offsets_and_sparse_triangle_vertices_preserve_clip_edges() {
    let g = row_index::tests::layout(&"abcdefghij".repeat(1000), f32::INFINITY, 1.5);
    let mut shape = egui::Shape::Text(TextShape::new(egui::Pos2::ZERO, g, egui::Color32::WHITE));
    shape.scale(1024.0);
    let egui::Shape::Text(mut text) = shape else {
        unreachable!()
    };
    let clip = egui::Rect::from_min_max(egui::pos2(0.25, 0.5), egui::pos2(320.75, 20.5));
    for fraction in [0.01, 0.5, 0.9999] {
        text.pos = egui::pos2(-text.galley.rect.width() * fraction + 0.25, -10.0);
        equivalent(&text, clip, false);
        equivalent(&text, clip, true);
    }
    // A valid mesh can have unused vertices and highly scattered index values.
    let mut g = (*text.galley).clone();
    let row = Arc::make_mut(&mut g.rows[0]);
    let mut mesh = Mesh::default();
    let mut vertices = row.visuals.mesh.vertices.clone();
    vertices.reverse();
    let count = vertices.len();
    mesh.vertices = vertices;
    mesh.indices = row
        .visuals
        .mesh
        .indices
        .iter()
        .map(|&i| count as u32 - 1 - i)
        .collect();
    row.visuals.mesh = mesh;
    row.visuals.mesh_cache.invalidate();
    text.galley = Arc::new(g);
    equivalent(&text, clip, true);
}

#[test]
fn batched_output_offsets_and_color_only_changes_preserve_visible_triangle_order() {
    let (_ctx, g) = styled(&"batched text ".repeat(1000), 1.25);
    let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 40.0));
    let mut shape = egui::Shape::Text(TextShape::new(
        egui::pos2(-g.rect.width() / 2.0, 0.0),
        g,
        egui::Color32::WHITE,
    ));
    let egui::Shape::Text(text) = &shape else {
        unreachable!()
    };
    equivalent(text, clip, true);
    let bytes = text.galley.rows[0].visuals.mesh_cache.bytes();
    egui::epaint::shape_transform::adjust_colors(&mut shape, |color| {
        if *color != egui::Color32::PLACEHOLDER {
            *color = egui::Color32::from_rgb(45, 90, 135);
        }
    });
    let egui::Shape::Text(mut text) = shape else {
        unreachable!()
    };
    assert_eq!(text.galley.rows[0].visuals.mesh_cache.bytes(), bytes);
    text.underline = egui::Stroke::new(1.25_f32, egui::Color32::RED);
    let mut prefix = Mesh::default();
    prefix.add_colored_rect(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0)),
        egui::Color32::GREEN,
    );
    let mut actual = prefix.clone();
    let mut reference = prefix;
    for (cull, out) in [(true, &mut actual), (false, &mut reference)] {
        let options = TessellationOptions {
            coarse_tessellation_culling: cull,
            ..Default::default()
        };
        let mut tess = Tessellator::new(1.25, options, [2048, 2048], vec![]);
        tess.set_clip_rect(clip);
        tess.tessellate_text(&text, out);
        assert!(out.is_valid());
    }
    assert_eq!(
        visible_triangles(&actual, clip),
        visible_triangles(&reference, clip)
    );
}
#[test]
fn nonfinite_geometry_and_nontriangle_index_lists_retain_the_uncached_path() {
    for nonfinite in [true, false] {
        let mut g =
            (*row_index::tests::layout(&"abcdefghij".repeat(1000), f32::INFINITY, 1.0)).clone();
        let row = Arc::make_mut(&mut g.rows[0]);
        if nonfinite {
            row.visuals.mesh.vertices[100].pos.x = f32::NAN;
        } else {
            row.visuals.mesh.indices.push(0);
        }
        let g = Arc::new(g);
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 40.0));
        let text = TextShape::new(egui::Pos2::ZERO, g.clone(), egui::Color32::WHITE);
        let actual = mesh(&text, clip, true, true);
        let reference = mesh(&text, clip, false, true);
        assert_eq!(actual.indices, reference.indices);
        assert_eq!(actual.vertices.len(), reference.vertices.len());
        assert_eq!(g.rows[0].visuals.mesh_cache.bytes(), 0);
        for (a, b) in actual.vertices.iter().zip(&reference.vertices) {
            assert_eq!(a.pos.x.to_bits(), b.pos.x.to_bits());
            assert_eq!(a.pos.y.to_bits(), b.pos.y.to_bits());
            assert_eq!(a.uv, b.uv);
            assert_eq!(a.color, b.color);
        }
    }
}

#[test]
fn block_culling_keeps_a_physical_pixel_guard_for_backend_scissor_rounding() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let mut g =
            (*row_index::tests::layout(&"abcdefghij".repeat(1000), f32::INFINITY, scale)).clone();
        let row = Arc::make_mut(&mut g.rows[0]);
        let clip = egui::Rect::from_min_max(egui::pos2(0.25, 0.25), egui::pos2(320.5, 20.5));
        let mut rectangles = Mesh::default();
        for n in 0..512 {
            let x = if n < 128 {
                clip.right() + 0.25 / scale
            } else {
                5.0
            };
            rectangles.add_colored_rect(
                egui::Rect::from_min_size(egui::pos2(x, 2.0), egui::vec2(0.25 / scale, 3.0)),
                egui::Color32::RED,
            );
        }
        row.visuals.mesh_bounds = rectangles.calc_bounds();
        row.visuals.mesh = rectangles;
        row.visuals.glyph_vertex_range = 0..0;
        row.visuals.mesh_cache.invalidate();
        let text = TextShape::new(egui::Pos2::ZERO, Arc::new(g), egui::Color32::WHITE);
        let guarded = clip.expand(1.0 / scale);
        assert_eq!(
            visible_triangles(&mesh(&text, clip, true, true), guarded),
            visible_triangles(&mesh(&text, clip, false, true), guarded)
        );
    }
}
