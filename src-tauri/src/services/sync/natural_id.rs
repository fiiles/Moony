//! Natural row IDs (spec §7.3 "same ID"): two devices that create the same ticker, split or
//! override offline derive the same UUIDv5, so the rows merge instead of colliding on the
//! table's unique key.

use uuid::Uuid;

/// Namespace of every natural ID. Never change it: it is part of the sync wire format.
pub const NAMESPACE: Uuid = Uuid::from_u128(0xb7a0c3e2_5d1f_4c8e_9a6b_2f3e1d0c9b8a);

/// Unit separator between the table name and the key parts.
const SEPARATOR: &str = "\u{1f}";

/// The ID of the row of `table` whose natural key is `key`, as a lowercase hyphenated UUID.
/// The table name is part of the hashed name, so equal keys in different tables differ.
pub fn natural_id(table: &str, key: &[&str]) -> String {
    let mut name = String::from(table);
    for part in key {
        name.push_str(SEPARATOR);
        name.push_str(part);
    }
    Uuid::new_v5(&NAMESPACE, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answers_pin_the_namespace_and_the_separator() {
        assert_eq!(
            natural_id("stock_investments", &["AAPL"]),
            "0dee614b-ed60-5962-ab0a-cc1d356a50ad"
        );
        assert_eq!(
            natural_id("stock_splits", &["NVDA", "1718064000"]),
            "89519a96-305b-52e9-9d93-4d828c336723"
        );
        assert_eq!(
            natural_id("learned_payees", &["ALBERT", "", "acc-1", "0"]),
            "325164e5-8176-51d7-86d9-f6dd04f3b363"
        );
    }

    #[test]
    fn the_table_name_is_part_of_the_id() {
        assert_ne!(
            natural_id("watched_stocks", &["AAPL"]),
            natural_id("stock_investments", &["AAPL"])
        );
    }
}
