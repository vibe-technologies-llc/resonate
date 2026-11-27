use resonate_lyrics::Lyrics;

pub fn within(lyrics: &Lyrics, line_at_most: usize, total_at_most: usize) {
    let lines = lyrics.lines();
    let longest = lines.iter().map(|line| line.text.len()).max().unwrap_or(0);
    let text: usize = lines.iter().map(|line| line.text.len()).sum();
    let words: usize = lines
        .iter()
        .flat_map(|line| &line.words)
        .map(|word| word.text.len())
        .sum();

    assert!(
        longest <= line_at_most,
        "a line of {longest} bytes, at most {line_at_most}"
    );
    assert!(
        text <= total_at_most,
        "{text} bytes of lines, at most {total_at_most}"
    );
    assert!(
        words <= total_at_most,
        "{words} bytes of words, at most {total_at_most}"
    );
}
