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
    "일시중단",   // Suspension (no space)
    "상장폐지",   // Delisting
    "이벤트",     // Event
    "지갑 점검",  // Wallet maintenance
    "지갑점검",   // Wallet maintenance (no space)
    "종료",       // Termination/ending (catches 거래지원 종료 = delisting)
    "중단",       // Halt/stop
];

const EXCLUSION_EN: &[&str] = &[
    "maintenance",
    "suspension",
    "delisting",
    "delist",
    "event",
    "wallet maintenance",
    "termination",
    "end of support",
];

// ── Primary keywords (need at least one) ──────────────────────────────

const PRIMARY_KR: &[&str] = &[
    "상장",           // Listing
    "거래 지원",      // Trading support (with space)
    "거래지원",       // Trading support (no space)
    "신규 상장",      // New listing (with space)
    "신규상장",       // New listing (no space)
    "마켓 추가",      // Market addition (with space)
    "마켓추가",       // Market addition (no space)
    "자산 추가",      // Asset addition (with space)
    "자산추가",       // Asset addition (no space)
    "거래 개시",      // Trading start (with space)
    "거래개시",       // Trading start (no space)
    "거래지원 안내",  // Trading support notice (Upbit's exact phrasing)
];

const PRIMARY_EN: &[&str] = &[
    "listing",
    "new coin",
    "new token",
    "trading support",
    "market support",
    "market addition",
];

// ── Secondary keywords (boost confidence) ─────────────────────────────

const SECONDARY_KR: &[&str] = &[
    "원화 마켓",  // KRW market (with space)
    "원화마켓",   // KRW market (no space)
    "원화",       // KRW
    "입출금",     // Deposit/withdrawal
    "거래 시작",  // Trading starts (with space)
    "거래시작",   // Trading starts (no space)
    "마켓",       // Market (appears in listing titles with market pairs)
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

    #[test]
    fn test_no_space_korean_keywords() {
        // Real CFG listing title — no space in 거래지원
        let result = is_listing_announcement("센트리퓨즈(CFG) 신규 거래지원 안내 (KRW, BTC, USDT 마켓)");
        assert!(result.is_listing, "Should detect listing with no-space 거래지원");
        assert!(result.confidence >= 0.6);
    }

    #[test]
    fn test_no_space_sinyu_sangjang() {
        let result = is_listing_announcement("신규상장 안내 - ABC 토큰");
        assert!(result.is_listing, "Should detect listing with no-space 신규상장");
        assert!(result.confidence >= 0.6);
    }

    #[test]
    fn test_exclusion_trading_support_ending() {
        // Real DENT delisting title — must NOT fire as a listing
        let result = is_listing_announcement("덴트(DENT) 거래지원 종료 안내(3/30 15:00)");
        assert!(!result.is_listing, "거래지원 종료 should be excluded (it's a delisting)");
    }

    #[test]
    fn test_exclusion_suspension_no_space() {
        let result = is_listing_announcement("디지털 자산 입출금 일시중단 (MATIC)");
        assert!(!result.is_listing, "일시중단 (no space) should be excluded");
    }

    /// Bulk test against real Upbit notice titles from the API.
    /// Every title must be correctly classified.
    #[test]
    fn test_real_upbit_titles() {
        // Should be DETECTED as listings
        let listings = vec![
            "센트리퓨즈(CFG) 신규 거래지원 안내 (KRW, BTC, USDT 마켓)",
            "원화(KRW) 마켓 디지털 자산 추가 (SOL)",
            "신규 상장 안내 - 원화 마켓 거래 시작 (ABC)",
            "비트코인캐시(BCH) 신규 거래지원 안내 (KRW 마켓)",
            "솔라나(SOL) KRW, BTC, USDT 마켓 추가",
        ];
        for title in &listings {
            let result = is_listing_announcement(title);
            assert!(result.is_listing, "MISSED listing: {}", title);
            assert!(result.confidence >= 0.6, "Low confidence for: {} (got {})", title, result.confidence);
        }

        // Should be REJECTED (not listings)
        let non_listings = vec![
            "덴트(DENT) 거래지원 종료 안내(3/30 15:00)",
            "Polygon 네트워크 하드포크에 따른 관련 디지털 자산 입출금 일시 중단 안내 (03/04 20:00 ~)",
            "시커(SKR) 프로젝트에 대해 공부하고, 시커폰 선물 받아가세요! (이벤트 종료)",
            "만트라(OM) 리브랜딩 및 토큰 스왑에 따른 입출금 및 거래 지원 일시 중단 안내 (입출금 중단 : 3/1 23:00 ~)",
            "업비트 개선 사항 : 주의 종목 경보 세분화 (주의/경고/위험)",
            "특정금융정보법에 따른 미신고 가상자산사업자와의 입출금 제한 및 유의사항",
            "불공정거래 예방조치 제도 안내",
            "업비트 공지사항 안내",
            "디지털 자산 입출금 일시 중단 (MATIC)",
            "BTC 거래 이벤트",
            "상장폐지 안내 (XYZ)",
            "지갑 점검 안내 (ETH)",
            "지갑점검 안내 (BTC)",
        ];
        for title in &non_listings {
            let result = is_listing_announcement(title);
            assert!(!result.is_listing, "FALSE POSITIVE: {}", title);
        }
    }
}
