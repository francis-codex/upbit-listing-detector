/// Keyword-based filtering for Upbit listing announcements.
///
/// Three-stage pipeline:
///   1. Exclusion keywords → reject immediately
///   2. Primary keywords   → need at least one match
///   3. Secondary keywords  → boost confidence score

// ── Exclusion keywords (reject if any match) ─────────────────────────

const EXCLUSION_KR: &[&str] = &[
    "점검",       // Maintenance
    "일시 중단",  // Suspension
    "상장폐지",   // Delisting
    "이벤트",     // Event
    "지갑 점검",  // Wallet maintenance
];

const EXCLUSION_EN: &[&str] = &[
    "maintenance",
    "suspension",
    "delisting",
    "event",
    "wallet maintenance",
];

// ── Primary keywords (need at least one) ──────────────────────────────

const PRIMARY_KR: &[&str] = &[
    "상장",       // Listing
    "거래 지원",  // Trading support
    "신규 상장",  // New listing
    "마켓 추가",  // Market addition
    "자산 추가",  // Asset addition
];

const PRIMARY_EN: &[&str] = &[
    "listing",
    "new coin",
    "new token",
    "trading support",
    "market addition",
];

// ── Secondary keywords (boost confidence) ─────────────────────────────

const SECONDARY_KR: &[&str] = &[
    "원화 마켓",  // KRW market
    "원화",       // KRW
    "입출금",     // Deposit/withdrawal
    "거래 시작",  // Trading starts
];

const SECONDARY_EN: &[&str] = &[
    "krw market",
    "btc market",
    "deposit",
    "withdrawal",
    "trading start",
];

/// Result of keyword analysis on a text.
#[derive(Debug, Clone)]
pub struct FilterResult {
    /// Whether the text is considered a listing announcement.
    pub is_listing: bool,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
    /// Primary keywords that matched.
    pub primary_matches: Vec<String>,
    /// Secondary keywords that matched.
    pub secondary_matches: Vec<String>,
    /// Exclusion keywords that matched (should be empty if is_listing is true).
    pub exclusion_matches: Vec<String>,
}

/// Analyze text to determine if it is a listing announcement.
///
/// Returns `(is_listing, confidence)` as a convenience, and
/// the full `FilterResult` for detailed inspection.
pub fn is_listing_announcement(text: &str) -> FilterResult {
    let lower = text.to_lowercase();

    // Stage 1: Exclusion check
    let exclusion_matches = find_matches(&lower, text, EXCLUSION_KR, EXCLUSION_EN);
    if !exclusion_matches.is_empty() {
        return FilterResult {
            is_listing: false,
            confidence: 0.0,
            primary_matches: vec![],
            secondary_matches: vec![],
            exclusion_matches,
        };
    }

    // Stage 2: Primary keyword check
    let primary_matches = find_matches(&lower, text, PRIMARY_KR, PRIMARY_EN);
    if primary_matches.is_empty() {
        return FilterResult {
            is_listing: false,
            confidence: 0.0,
            primary_matches: vec![],
            secondary_matches: vec![],
            exclusion_matches: vec![],
        };
    }

    // Stage 3: Secondary keyword boost
    let secondary_matches = find_matches(&lower, text, SECONDARY_KR, SECONDARY_EN);

    // Confidence calculation
    let primary_score = (primary_matches.len() as f32 * 0.5).min(1.0);
    let secondary_score = secondary_matches.len() as f32 * 0.1;
    let confidence = (primary_score + secondary_score).min(1.0);

    FilterResult {
        is_listing: confidence >= 0.6,
        confidence,
        primary_matches,
        secondary_matches,
        exclusion_matches: vec![],
    }
}

/// Find all matching keywords from Korean and English lists.
fn find_matches(lower: &str, original: &str, kr: &[&str], en: &[&str]) -> Vec<String> {
    let mut matches = Vec::new();

    // Korean keywords checked against original text (case-insensitive not needed for Korean)
    for &kw in kr {
        if original.contains(kw) {
            matches.push(kw.to_string());
        }
    }

    // English keywords checked against lowercased text
    for &kw in en {
        if lower.contains(kw) {
            matches.push(kw.to_string());
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_listing_detection_korean() {
        let result = is_listing_announcement("원화(KRW) 마켓 디지털 자산 추가 (SOL)");
        assert!(result.is_listing);
        assert!(result.confidence >= 0.6);
        assert!(!result.primary_matches.is_empty());
    }

    #[test]
    fn test_listing_detection_english() {
        let result = is_listing_announcement("New listing: SOL trading support on KRW market");
        assert!(result.is_listing);
        assert!(result.confidence >= 0.6);
    }

    #[test]
    fn test_exclusion_maintenance() {
        let result = is_listing_announcement("디지털 자산 입출금 일시 중단 (MATIC)");
        assert!(!result.is_listing);
        assert_eq!(result.confidence, 0.0);
        assert!(!result.exclusion_matches.is_empty());
    }

    #[test]
    fn test_exclusion_event() {
        let result = is_listing_announcement("BTC 거래 이벤트");
        assert!(!result.is_listing);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_exclusion_delisting() {
        let result = is_listing_announcement("상장폐지 안내 (XYZ)");
        assert!(!result.is_listing);
    }

    #[test]
    fn test_no_primary_keyword() {
        let result = is_listing_announcement("업비트 공지사항 안내");
        assert!(!result.is_listing);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_high_confidence_with_secondary() {
        let result = is_listing_announcement("신규 상장 안내 - 원화 마켓 거래 시작 (ABC)");
        assert!(result.is_listing);
        assert!(result.confidence >= 0.7);
    }

    #[test]
    fn test_multiple_primary_keywords() {
        let result = is_listing_announcement("신규 상장 마켓 추가 SOL");
        assert!(result.is_listing);
        assert!(result.confidence >= 0.6);
    }
}
