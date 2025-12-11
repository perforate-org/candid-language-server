use std::sync::Arc;

use candid_language_server::lsp::{
    completion::bench_support::{CompletionBenchFixture, CursorContextSnapshot},
    config::ServiceSnippetStyle,
};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

const SAMPLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/hover_sample.did"
));

fn bench_cursor_context(c: &mut Criterion) {
    let fixture = Arc::new(CompletionBenchFixture::load(SAMPLE).expect("fixture"));
    let offset = fixture.offset_of("service Api").expect("service label");
    let fixture_ref = fixture.clone();
    c.bench_function("cursor_context_service_block", move |b| {
        b.iter(|| {
            let snapshot: CursorContextSnapshot = fixture_ref.cursor_context_snapshot(offset);
            black_box(snapshot);
        });
    });
}

fn bench_value_completion(c: &mut Criterion) {
    let fixture = Arc::new(CompletionBenchFixture::load(SAMPLE).expect("fixture"));
    let offset = fixture.offset_of("set_value").expect("value placeholder");
    let fixture_ref = fixture.clone();
    c.bench_function("completion_value_block", move |b| {
        b.iter(|| {
            let count = fixture_ref.completion_items_at(offset, ServiceSnippetStyle::Call);
            black_box(count);
        });
    });
}

fn bench_service_snippets(c: &mut Criterion) {
    let fixture = Arc::new(CompletionBenchFixture::load(SAMPLE).expect("fixture"));
    let offset = fixture.offset_of("service Api").expect("service header");
    let fixture_ref = fixture.clone();
    c.bench_function("completion_service_snippets", move |b| {
        b.iter(|| {
            let count = fixture_ref.completion_items_at(offset, ServiceSnippetStyle::Await);
            black_box(count);
        });
    });
}

criterion_group!(
    completion_benches,
    bench_cursor_context,
    bench_value_completion,
    bench_service_snippets
);
criterion_main!(completion_benches);
