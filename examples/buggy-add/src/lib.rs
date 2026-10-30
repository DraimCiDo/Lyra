pub fn add(left: i32, right: i32) -> i32 {
    left - right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds_positive_numbers() {
        assert_eq!(add(2, 3), 5);
    }

    #[test]
    fn adds_negative_numbers() {
        assert_eq!(add(-4, -2), -6);
    }
}
