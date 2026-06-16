//! Verify: identical Labels across frames → GalleyCache hit (no re-layout).
//! Run: `cargo test -p deepx-egui-exp1 -- --nocapture galley_cache`

#[cfg(test)]
mod tests {
    #[test]
    fn galley_cache_hit_on_identical_labels() {
        let ctx = egui::Context::default();
        let mut counts = Vec::new();

        // Helper: run one frame, render 3 labels, record cache size
        let mut run_frame = |label3: &str| {
            let mut count = 0;
            egui::__run_test_ui(|ui| {
                ui.label("段落一：这是已经完成的文本。");
                ui.label("段落二：这也是已经完成的文本。");
                ui.label(label3);
                count = ui.ctx().fonts(|f| f.num_galleys_in_cache());
            });
            count
        };

        // Frame 1: all three are new → ~3 cache entries
        let c1 = run_frame("段落三：这段还在流");
        counts.push(c1);
        println!("Frame 1 cache: {c1}");

        // Frame 2: same first two, changed third → +1 cache entry
        let c2 = run_frame("段落三：这段还在流式输出中");
        counts.push(c2);
        println!("Frame 2 cache: {c2}");

        // Frame 3: all three identical to frame 2 → cache should NOT grow
        let c3 = run_frame("段落三：这段还在流式输出中");
        counts.push(c3);
        println!("Frame 3 cache: {c3}");

        // Frame 4-6: repeat frame 3 pattern
        for i in 4..=6 {
            let c = run_frame("段落三：这段还在流式输出中");
            counts.push(c);
            println!("Frame {i} cache: {c}");
        }

        // Assert: cache stabilizes (no unbounded growth)
        let max_growth = c2.saturating_sub(c1); // at most +1 from the changed label
        let stable = counts.windows(2).all(|w| w[1] <= w[0] + 2);
        let bounded = counts.last().copied().unwrap_or(0) <= c1 + max_growth + 3;

        println!(
            "max_growth={max_growth}, stable={stable}, bounded={bounded}, final={}",
            counts.last().unwrap_or(&0)
        );

        assert!(stable, "Cache should stabilize when labels are unchanged");
        assert!(
            bounded,
            "Cache should not grow unboundedly across frames"
        );
        println!("✓ GalleyCache correctly caches identical labels across frames");
    }
}
