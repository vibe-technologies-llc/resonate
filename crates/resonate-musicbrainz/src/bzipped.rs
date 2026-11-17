use std::io::Read;

#[cfg(test)]
mod tests;

pub(crate) fn decoded<R: Read>(packed: R) -> impl Read {
    mibz2::BzReader::new(packed)
}

pub(crate) fn decoded_across<R: Read + Send + 'static>(packed: R, threads: usize) -> impl Read {
    mibz2::BzReader::parallel(packed, threads)
}

#[cfg(any(test, fuzzing))]
pub(crate) fn read_whole(decoded: impl Read, at_most: u64) -> std::io::Result<Vec<u8>> {
    let mut whole = Vec::new();
    decoded.take(at_most).read_to_end(&mut whole)?;
    Ok(whole)
}
