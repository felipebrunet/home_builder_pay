pub fn monero_fn() -> u32 {
    2 + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_plus_two() {
        assert_eq!(monero_fn(), 4);
    }
}
