// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! A demo.

fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}

fn main() {
    println!("{}", greeting("world"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_by_name() {
        assert_eq!(greeting("world"), "Hello, world!");
    }
}
