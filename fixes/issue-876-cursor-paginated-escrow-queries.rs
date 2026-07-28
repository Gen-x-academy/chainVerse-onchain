//! Fix for #876: enforce a max page size and stable cursor on escrow
//! result queries instead of returning full unbounded vectors.
pub struct EscrowRecord {
    pub id: u64,
    pub party: String,
}

pub const MAX_PAGE_SIZE: usize = 50;

pub fn query_by_party<'a>(
    escrows: &'a [EscrowRecord],
    party: &str,
    after_id: Option<u64>,
    limit: usize,
) -> Vec<&'a EscrowRecord> {
    let bounded_limit = limit.min(MAX_PAGE_SIZE);
    escrows
        .iter()
        .filter(|e| e.party == party && after_id.map_or(true, |a| e.id > a))
        .take(bounded_limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_size_is_capped_regardless_of_requested_limit() {
        let escrows: Vec<EscrowRecord> = (1..=100)
            .map(|id| EscrowRecord { id, party: "buyer-a".to_string() })
            .collect();
        let page = query_by_party(&escrows, "buyer-a", None, 1000);
        assert_eq!(page.len(), MAX_PAGE_SIZE);
        let next = query_by_party(&escrows, "buyer-a", Some(page.last().unwrap().id), 1000);
        assert_eq!(next[0].id, 51);
    }
}
