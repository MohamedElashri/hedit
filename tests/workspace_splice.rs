use hedit::workspace::splice;

#[test]
fn block_comparison_preserves_exact_utf8_splices_across_alignment_boundaries() {
    let old = "line 🦀 שלום\r\n".repeat(100);
    let positions: Vec<_> = old
        .char_indices()
        .map(|(i, _)| i)
        .chain([old.len()])
        .collect();
    for a in (0..positions.len()).step_by(17) {
        for length in [0, 1, 7, 64, 127] {
            let b = (a + length).min(positions.len() - 1);
            for insertion in ["", "x", "🦀\r\nשלום"] {
                let mut new = old.clone();
                new.replace_range(positions[a]..positions[b], insertion);
                let (start, end, inserted) = splice(&old, &new);
                let mut result = old.clone();
                result.replace_range(start..end, inserted);
                assert_eq!(result, new);
                // Compare the original scalar reference, including minimality.
                let mut prefix = old
                    .bytes()
                    .zip(new.bytes())
                    .take_while(|(a, b)| a == b)
                    .count();
                while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
                    prefix -= 1;
                }
                let mut suffix = old[prefix..]
                    .bytes()
                    .rev()
                    .zip(new[prefix..].bytes().rev())
                    .take_while(|(a, b)| a == b)
                    .count();
                while !old.is_char_boundary(old.len() - suffix)
                    || !new.is_char_boundary(new.len() - suffix)
                {
                    suffix -= 1;
                }
                assert_eq!(
                    (start, end, inserted),
                    (prefix, old.len() - suffix, &new[prefix..new.len() - suffix])
                );
            }
        }
    }
}
