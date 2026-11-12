use crate::words;

pub struct Presented {
    pub generic_name: &'static str,
    pub summary: &'static str,
    pub keywords: &'static str,
    pub description: [&'static str; 3],
}

pub fn presented() -> Presented {
    Presented {
        generic_name: words::presented::generic_name(),
        summary: words::presented::summary(),
        keywords: words::presented::keywords(),
        description: [
            words::presented::what_it_is(),
            words::presented::what_it_keeps(),
            words::presented::what_it_reads(),
        ],
    }
}
