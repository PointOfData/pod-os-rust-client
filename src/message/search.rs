/// Pattern discriminant for the Pod-OS search DSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    FastPattern,
    QuestionMark,
    Asterisk,
    CharSet,
    CharRange,
    Regexp,
    Eq,
    Ne,
    Le,
    Lt,
    Ge,
    Gt,
    Distance,
    RangeEq,
    RangeNe,
    IntEq,
    IntNe,
    IntLe,
    IntLt,
    IntGe,
    IntGt,
    IntRange,
    IntRangeNe,
    DblEq,
    DblNe,
    DblLe,
    DblLt,
    DblGe,
    DblGt,
    DblRange,
    DblRangeNe,
}

#[derive(Debug, Clone, Default)]
pub struct Pattern {
    pub r#type:     PatternType,
    pub low_value:  String,
    pub high_value: String,
}

impl Default for PatternType {
    fn default() -> Self { PatternType::FastPattern }
}

#[derive(Debug, Clone, Default)]
pub struct PatternMatch {
    pub matched:    bool,
    pub value:      String,
    pub position:   i32,
    pub confidence: f64,
}

#[derive(Debug, Clone, Default)]
pub struct PatternSearch {
    pub patterns:       Vec<Pattern>,
    pub operator:       String,
    pub case_sensitive: bool,
    pub whole_word:     bool,
}

#[derive(Debug, Clone, Default)]
pub struct FastPattern {
    pub pattern:    String,
    pub low_value:  String,
    pub high_value: String,
}

#[derive(Debug, Clone, Default)]
pub struct CharSetPattern {
    pub characters: String,
    pub range:      String,
    pub inclusive:  bool,
}

#[derive(Debug, Clone, Default)]
pub struct DistancePattern {
    pub comparison_string: String,
    pub max_distance:      i32,
}

#[derive(Debug, Clone, Default)]
pub struct RangePattern {
    pub low_value:  String,
    pub high_value: String,
    pub inclusive:  bool,
}

#[derive(Debug, Clone, Default)]
pub struct SearchClause {
    pub clause: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchBranch {
    pub branch: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchAction {
    pub action: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResults {
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    pub total_event_hits:    i32,
    pub returned_event_hits: i32,
    pub set_link_count:      i32,
    pub start_result:        String,
    pub end_result:          String,
}
