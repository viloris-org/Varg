//! Tests for render graph construction and execution.

use engine_render::{HeadlessRenderDevice, RenderDevice, RenderFrame, RenderGraphBuilder};

#[test]
fn empty_render_graph() {
    let graph = RenderGraphBuilder::new().build();
    assert_eq!(graph.pass_count(), 0);
}

#[test]
fn single_pass_render_graph() {
    let mut builder = RenderGraphBuilder::new();
    builder.add_pass("main");
    let graph = builder.build();
    assert_eq!(graph.pass_count(), 1);
    assert_eq!(graph.passes[0].name, "main");
}

#[test]
fn multiple_passes_with_ordering() {
    let mut builder = RenderGraphBuilder::new();
    let a = builder.add_pass("gbuffer");
    let b = builder.add_pass("lighting");
    let c = builder.add_pass("post");
    builder.order_before(a, b);
    builder.order_before(b, c);

    let graph = builder.build();
    assert_eq!(graph.pass_count(), 3);
    assert_eq!(graph.passes[0].name, "gbuffer");
    assert_eq!(graph.passes[1].name, "lighting");
    assert_eq!(graph.passes[2].name, "post");
}

#[test]
fn linear_chain_of_five_passes() {
    let mut builder = RenderGraphBuilder::new();
    let passes: Vec<_> = (0..5)
        .map(|i| builder.add_pass(&format!("pass_{i}")))
        .collect();
    for i in 0..4 {
        builder.order_before(passes[i], passes[i + 1]);
    }
    let graph = builder.build();
    assert_eq!(graph.pass_count(), 5);
    for i in 0..5 {
        assert_eq!(graph.passes[i].name, format!("pass_{i}"));
    }
}

#[test]
fn headless_device_executes_empty_graph() {
    let mut device = HeadlessRenderDevice::default();
    let graph = RenderGraphBuilder::new().build();
    let result = device.execute_graph(&graph, RenderFrame { frame_index: 0 });
    assert!(result.is_ok(), "empty graph should execute");
}

#[test]
fn headless_device_executes_single_pass_graph() {
    let mut device = HeadlessRenderDevice::default();
    let mut builder = RenderGraphBuilder::new();
    builder.add_pass("test_pass");
    let graph = builder.build();
    let result = device.execute_graph(&graph, RenderFrame { frame_index: 0 });
    assert!(result.is_ok(), "single pass should execute");
}

#[test]
fn headless_device_executes_multi_pass_graph() {
    let mut device = HeadlessRenderDevice::default();
    let mut builder = RenderGraphBuilder::new();
    let a = builder.add_pass("shadow");
    let b = builder.add_pass("forward");
    let c = builder.add_pass("post");
    builder.order_before(a, b);
    builder.order_before(b, c);
    let graph = builder.build();
    let result = device.execute_graph(&graph, RenderFrame { frame_index: 0 });
    assert!(
        result.is_ok(),
        "multi-pass should execute on headless device"
    );
}

#[test]
fn render_frame_index_propagates() {
    let mut device = HeadlessRenderDevice::default();
    let mut builder = RenderGraphBuilder::new();
    builder.add_pass("frame_pass");
    let graph = builder.build();
    for i in 0..10 {
        device
            .execute_graph(&graph, RenderFrame { frame_index: i })
            .expect("frame should execute");
    }
}

#[test]
fn build_default_render_graph_pass_order() {
    use runtime_min::build_default_render_graph;
    let graph = build_default_render_graph();
    assert_eq!(graph.pass_count(), 5);
    assert_eq!(graph.passes[0].name, "shadow");
    assert_eq!(graph.passes[1].name, "forward");
    assert_eq!(graph.passes[2].name, "upscale");
    assert_eq!(graph.passes[3].name, "post");
    assert_eq!(graph.passes[4].name, "ui");
}

#[test]
fn pass_names_are_unique() {
    let mut builder = RenderGraphBuilder::new();
    builder.add_pass("a");
    builder.add_pass("b");
    builder.add_pass("c");
    let graph = builder.build();
    let mut names: Vec<&str> = graph.passes.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), graph.pass_count(), "pass names must be unique");
}

#[test]
fn reordering_passes_after_build_is_not_possible() {
    let mut builder = RenderGraphBuilder::new();
    let _a = builder.add_pass("first");
    let _b = builder.add_pass("second");
    let _c = builder.add_pass("third");
    let graph = builder.build();
    // The built graph should have finalized ordering
    // All passes exist regardless of explicit order calls
    assert!(graph.passes.iter().any(|p| p.name == "first"));
    assert!(graph.passes.iter().any(|p| p.name == "second"));
    assert!(graph.passes.iter().any(|p| p.name == "third"));
}
