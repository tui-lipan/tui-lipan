pub(crate) fn round_mul_div(numer: usize, multiplier: usize, divisor: usize) -> usize {
    if divisor == 0 {
        return 0;
    }

    let numer = numer as u128;
    let multiplier = multiplier as u128;
    let divisor = divisor as u128;
    ((numer.saturating_mul(multiplier) + divisor / 2) / divisor) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divisor_zero_returns_zero() {
        assert_eq!(round_mul_div(5, 3, 0), 0);
    }

    #[test]
    fn numer_zero_returns_zero() {
        assert_eq!(round_mul_div(0, 5, 3), 0);
    }

    #[test]
    fn multiplier_zero_returns_zero() {
        assert_eq!(round_mul_div(5, 0, 3), 0);
    }

    #[test]
    fn exact_division() {
        assert_eq!(round_mul_div(6, 2, 3), 4);
    }

    #[test]
    fn rounds_up() {
        // 5 * 1 / 3 = 1.667 → rounds to 2
        assert_eq!(round_mul_div(5, 1, 3), 2);
    }

    #[test]
    fn rounds_down() {
        // 4 * 1 / 3 = 1.333 → rounds to 1
        assert_eq!(round_mul_div(4, 1, 3), 1);
    }

    #[test]
    fn large_values_no_overflow() {
        assert_eq!(round_mul_div(usize::MAX, 1, 1), usize::MAX);
    }

    #[test]
    fn large_multiplication_no_overflow() {
        // (usize::MAX / 2) * 2 / 1 - u128 intermediate handles it
        let result = round_mul_div(usize::MAX / 2, 2, 1);
        assert_eq!(result, usize::MAX - 1);
    }
}
