//! Query Parser
//!
//! Parses Chronicle Query Language (CQL) strings into Query AST.
//!
//! # Supported Syntax
//!
//! ```text
//! SELECT metric [, metric2, ...]
//! [WHERE time >= now() - 7d]
//! [AND tags.location = 'office']
//! [GROUP BY day|hour|week|month]
//! [LIMIT n]
//! ```
//!
//! # Examples
//!
//! ```text
//! SELECT mood
//! SELECT mood WHERE time >= now() - 7d
//! SELECT AVG(mood) GROUP BY day
//! SELECT mood, energy WHERE tags.location = 'office'
//! ```

use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while, take_while1},
    character::complete::{char, digit1, multispace0, multispace1},
    combinator::{map, map_res, opt, recognize, value},
    multi::separated_list1,
    sequence::{delimited, pair, tuple},
    IResult,
};

use crate::query::ast::*;
use crate::query::error::{QueryError, QueryResult};
use crate::storage::TimeRange;
use chrono::Utc;

/// Parse a query string into a Query AST
pub fn parse_query(input: &str) -> QueryResult<Query> {
    let input = input.trim();

    match parse_full_query(input) {
        Ok((remaining, query)) => {
            if remaining.trim().is_empty() {
                Ok(query)
            } else {
                Err(QueryError::Parse(format!(
                    "Unexpected input after query: '{}'",
                    remaining.trim()
                )))
            }
        }
        Err(e) => Err(QueryError::Parse(format!("Parse error: {:?}", e))),
    }
}

/// Parse the full query
fn parse_full_query(input: &str) -> IResult<&str, Query> {
    let (input, _) = multispace0(input)?;
    let (input, select) = parse_select_clause(input)?;
    let (input, _) = multispace0(input)?;
    let (input, where_clause) = opt(parse_where_clause)(input)?;
    let (input, _) = multispace0(input)?;
    let (input, group_by) = opt(parse_group_by_clause)(input)?;
    let (input, _) = multispace0(input)?;
    let (input, limit) = opt(parse_limit_clause)(input)?;
    let (input, _) = multispace0(input)?;

    // Extract time range and filters from WHERE clause
    let (time_range, filters) = match where_clause {
        Some((tr, f)) => (tr, f),
        None => (None, Vec::new()),
    };

    Ok((
        input,
        Query {
            select,
            time_range: time_range.unwrap_or_else(|| TimeRange::last_days(7)),
            filters,
            group_by,
            limit,
        },
    ))
}

/// Parse SELECT clause
fn parse_select_clause(input: &str) -> IResult<&str, Vec<SelectItem>> {
    let (input, _) = tag_no_case("SELECT")(input)?;
    let (input, _) = multispace1(input)?;

    // Handle SELECT *
    if let Ok((input, _)) = char::<&str, nom::error::Error<&str>>('*')(input) {
        return Ok((input, vec![SelectItem::new("*")]));
    }

    separated_list1(
        delimited(multispace0, char(','), multispace0),
        parse_select_item,
    )(input)
}

/// Parse a single SELECT item (metric or aggregated metric)
fn parse_select_item(input: &str) -> IResult<&str, SelectItem> {
    alt((parse_aggregated_item, parse_simple_item))(input)
}

/// Parse an aggregated item like AVG(mood)
fn parse_aggregated_item(input: &str) -> IResult<&str, SelectItem> {
    let (input, agg) = parse_aggregation_func(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char('(')(input)?;
    let (input, _) = multispace0(input)?;
    let (input, metric) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = char(')')(input)?;
    let (input, alias) = opt(parse_alias)(input)?;

    Ok((
        input,
        SelectItem {
            metric: metric.to_string(),
            aggregation: Some(agg),
            alias,
        },
    ))
}

/// Parse a simple metric item
fn parse_simple_item(input: &str) -> IResult<&str, SelectItem> {
    let (input, metric) = parse_identifier(input)?;
    let (input, alias) = opt(parse_alias)(input)?;

    Ok((
        input,
        SelectItem {
            metric: metric.to_string(),
            aggregation: None,
            alias,
        },
    ))
}

/// Parse AS alias clause
fn parse_alias(input: &str) -> IResult<&str, String> {
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("AS")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, alias) = parse_identifier(input)?;
    Ok((input, alias.to_string()))
}

/// Parse aggregation function name
fn parse_aggregation_func(input: &str) -> IResult<&str, AggregationFunc> {
    alt((
        value(AggregationFunc::Avg, alt((tag_no_case("AVG"), tag_no_case("AVERAGE")))),
        value(AggregationFunc::Sum, tag_no_case("SUM")),
        value(AggregationFunc::Min, tag_no_case("MIN")),
        value(AggregationFunc::Max, tag_no_case("MAX")),
        value(AggregationFunc::Count, tag_no_case("COUNT")),
        value(AggregationFunc::Last, tag_no_case("LAST")),
        value(AggregationFunc::First, tag_no_case("FIRST")),
    ))(input)
}

/// Parse WHERE clause
fn parse_where_clause(input: &str) -> IResult<&str, (Option<TimeRange>, Vec<Filter>)> {
    let (input, _) = tag_no_case("WHERE")(input)?;
    let (input, _) = multispace1(input)?;

    let (input, conditions) = separated_list1(
        delimited(
            multispace0,
            tag_no_case("AND"),
            multispace1,
        ),
        parse_condition,
    )(input)?;

    // Separate time conditions from filter conditions
    let mut time_range: Option<TimeRange> = None;
    let mut filters = Vec::new();

    for condition in conditions {
        match condition {
            Condition::TimeRange(tr) => {
                time_range = Some(tr);
            }
            Condition::Filter(f) => {
                filters.push(f);
            }
        }
    }

    Ok((input, (time_range, filters)))
}

/// Condition in WHERE clause (either time range or filter)
enum Condition {
    TimeRange(TimeRange),
    Filter(Filter),
}

/// Parse a single condition
fn parse_condition(input: &str) -> IResult<&str, Condition> {
    alt((
        map(parse_time_condition, Condition::TimeRange),
        map(parse_filter_condition, Condition::Filter),
    ))(input)
}

/// Parse time condition like "time >= now() - 7d"
fn parse_time_condition(input: &str) -> IResult<&str, TimeRange> {
    let (input, _) = tag_no_case("time")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, op) = parse_operator(input)?;
    let (input, _) = multispace0(input)?;
    let (input, timestamp) = parse_time_expression(input)?;

    // Convert to TimeRange based on operator
    let now = Utc::now().timestamp_millis();
    let range = match op {
        Operator::Gte | Operator::Gt => TimeRange::new(timestamp, now + 1000),
        Operator::Lte | Operator::Lt => TimeRange::new(0, timestamp),
        Operator::Eq => TimeRange::new(timestamp, timestamp + 1),
        Operator::Ne => TimeRange::new(0, now + 1000), // Can't represent "not equal" as range
    };

    Ok((input, range))
}

/// Parse time expression like "now() - 7d"
fn parse_time_expression(input: &str) -> IResult<&str, i64> {
    alt((
        parse_relative_time,
        parse_absolute_time,
    ))(input)
}

/// Parse relative time like "now() - 7d"
fn parse_relative_time(input: &str) -> IResult<&str, i64> {
    let (input, _) = tag_no_case("now()")(input)?;
    let (input, _) = multispace0(input)?;

    // Optional offset
    let (input, offset) = opt(|input| {
        let (input, _) = char('-')(input)?;
        let (input, _) = multispace0(input)?;
        parse_duration(input)
    })(input)?;

    let now = Utc::now().timestamp_millis();
    let result = match offset {
        Some(dur) => now - dur,
        None => now,
    };

    Ok((input, result))
}

/// Parse absolute time (quoted string or timestamp)
fn parse_absolute_time(input: &str) -> IResult<&str, i64> {
    alt((
        // Quoted datetime string
        map(parse_quoted_string, |s| {
            // Try to parse as ISO 8601
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or(0)
        }),
        // Unix timestamp in milliseconds
        map_res(digit1, |s: &str| s.parse::<i64>()),
    ))(input)
}

/// Parse duration like "7d", "24h", "30m"
fn parse_duration(input: &str) -> IResult<&str, i64> {
    let (input, num) = map_res(digit1, |s: &str| s.parse::<i64>())(input)?;
    let (input, unit) = alt((
        value(24 * 60 * 60 * 1000i64, alt((tag("d"), tag("D")))),
        value(60 * 60 * 1000i64, alt((tag("h"), tag("H")))),
        value(60 * 1000i64, alt((tag("m"), tag("M")))),
        value(1000i64, alt((tag("s"), tag("S")))),
    ))(input)?;

    Ok((input, num * unit))
}

/// Parse filter condition like "tags.location = 'office'"
fn parse_filter_condition(input: &str) -> IResult<&str, Filter> {
    alt((
        parse_tag_filter,
        parse_value_filter,
    ))(input)
}

/// Parse tag filter like "tags.location = 'office'"
fn parse_tag_filter(input: &str) -> IResult<&str, Filter> {
    let (input, _) = tag_no_case("tags.")(input)?;
    let (input, key) = parse_identifier(input)?;
    let (input, _) = multispace0(input)?;
    let (input, op) = parse_operator(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_quoted_string(input)?;

    Ok((
        input,
        Filter {
            field: FilterField::Tag(key.to_string()),
            op,
            value: FilterValue::String(value),
        },
    ))
}

/// Parse value filter like "value > 5"
fn parse_value_filter(input: &str) -> IResult<&str, Filter> {
    let (input, _) = tag_no_case("value")(input)?;
    let (input, _) = multispace0(input)?;
    let (input, op) = parse_operator(input)?;
    let (input, _) = multispace0(input)?;
    let (input, value) = parse_number(input)?;

    Ok((
        input,
        Filter {
            field: FilterField::Value,
            op,
            value: FilterValue::Number(value),
        },
    ))
}

/// Parse GROUP BY clause
fn parse_group_by_clause(input: &str) -> IResult<&str, GroupByClause> {
    let (input, _) = tag_no_case("GROUP")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, _) = tag_no_case("BY")(input)?;
    let (input, _) = multispace1(input)?;
    let (input, interval) = parse_group_by_interval(input)?;

    Ok((input, GroupByClause { interval }))
}

/// Parse GROUP BY interval
fn parse_group_by_interval(input: &str) -> IResult<&str, GroupByInterval> {
    alt((
        value(GroupByInterval::Hour, tag_no_case("hour")),
        value(GroupByInterval::Day, tag_no_case("day")),
        value(GroupByInterval::Week, tag_no_case("week")),
        value(GroupByInterval::Month, tag_no_case("month")),
    ))(input)
}

/// Parse LIMIT clause
fn parse_limit_clause(input: &str) -> IResult<&str, usize> {
    let (input, _) = tag_no_case("LIMIT")(input)?;
    let (input, _) = multispace1(input)?;
    map_res(digit1, |s: &str| s.parse::<usize>())(input)
}

/// Parse comparison operator
fn parse_operator(input: &str) -> IResult<&str, Operator> {
    alt((
        value(Operator::Gte, tag(">=")),
        value(Operator::Lte, tag("<=")),
        value(Operator::Ne, alt((tag("!="), tag("<>")))),
        value(Operator::Gt, tag(">")),
        value(Operator::Lt, tag("<")),
        value(Operator::Eq, alt((tag("=="), tag("=")))),
    ))(input)
}

/// Parse identifier (metric name, tag key, etc.)
fn parse_identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        take_while1(|c: char| c.is_alphabetic() || c == '_'),
        take_while(|c: char| c.is_alphanumeric() || c == '_'),
    ))(input)
}

/// Parse quoted string
fn parse_quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = char('\'')(input)?;
    let (input, content) = take_while(|c| c != '\'')(input)?;
    let (input, _) = char('\'')(input)?;
    Ok((input, content.to_string()))
}

/// Parse floating point number
fn parse_number(input: &str) -> IResult<&str, f64> {
    map_res(
        recognize(tuple((
            opt(char('-')),
            digit1,
            opt(pair(char('.'), digit1)),
        ))),
        |s: &str| s.parse::<f64>(),
    )(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_select() {
        let query = parse_query("SELECT mood").unwrap();
        assert_eq!(query.select.len(), 1);
        assert_eq!(query.select[0].metric, "mood");
        assert!(query.select[0].aggregation.is_none());
    }

    #[test]
    fn test_parse_multiple_select() {
        let query = parse_query("SELECT mood, energy, focus").unwrap();
        assert_eq!(query.select.len(), 3);
        assert_eq!(query.select[0].metric, "mood");
        assert_eq!(query.select[1].metric, "energy");
        assert_eq!(query.select[2].metric, "focus");
    }

    #[test]
    fn test_parse_aggregated_select() {
        let query = parse_query("SELECT AVG(mood)").unwrap();
        assert_eq!(query.select.len(), 1);
        assert_eq!(query.select[0].metric, "mood");
        assert_eq!(query.select[0].aggregation, Some(AggregationFunc::Avg));
    }

    #[test]
    fn test_parse_with_alias() {
        let query = parse_query("SELECT AVG(mood) AS daily_mood").unwrap();
        assert_eq!(query.select[0].alias, Some("daily_mood".to_string()));
    }

    #[test]
    fn test_parse_where_time() {
        let query = parse_query("SELECT mood WHERE time >= now() - 7d").unwrap();
        assert_eq!(query.select[0].metric, "mood");
        // Time range should be approximately 7 days ago
        let now = Utc::now().timestamp_millis();
        let seven_days_ms = 7 * 24 * 60 * 60 * 1000;
        assert!(query.time_range.start < now);
        assert!(query.time_range.start > now - seven_days_ms - 1000);
    }

    #[test]
    fn test_parse_group_by() {
        let query = parse_query("SELECT AVG(mood) GROUP BY day").unwrap();
        assert!(query.group_by.is_some());
        assert_eq!(query.group_by.unwrap().interval, GroupByInterval::Day);
    }

    #[test]
    fn test_parse_limit() {
        let query = parse_query("SELECT mood LIMIT 100").unwrap();
        assert_eq!(query.limit, Some(100));
    }

    #[test]
    fn test_parse_full_query() {
        let query = parse_query(
            "SELECT AVG(mood) AS daily_mood WHERE time >= now() - 30d GROUP BY day LIMIT 30",
        )
        .unwrap();

        assert_eq!(query.select[0].metric, "mood");
        assert_eq!(query.select[0].aggregation, Some(AggregationFunc::Avg));
        assert_eq!(query.select[0].alias, Some("daily_mood".to_string()));
        assert_eq!(query.group_by.unwrap().interval, GroupByInterval::Day);
        assert_eq!(query.limit, Some(30));
    }

    #[test]
    fn test_parse_tag_filter() {
        let query = parse_query("SELECT mood WHERE tags.location = 'office'").unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, FilterField::Tag("location".to_string()));
    }

    #[test]
    fn test_parse_multiple_conditions() {
        let query = parse_query(
            "SELECT mood WHERE time >= now() - 7d AND tags.location = 'office'",
        )
        .unwrap();

        // Time range should be set from time condition
        let now = Utc::now().timestamp_millis();
        assert!(query.time_range.start < now);

        // Tag filter should be captured
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, FilterField::Tag("location".to_string()));
    }

    #[test]
    fn test_parse_case_insensitive() {
        let query = parse_query("select avg(mood) where time >= now() - 7d group by day").unwrap();
        assert_eq!(query.select[0].aggregation, Some(AggregationFunc::Avg));
        assert!(query.group_by.is_some());
    }

    #[test]
    fn test_parse_value_filter() {
        let query = parse_query("SELECT mood WHERE value >= 5.0").unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, FilterField::Value);
        assert_eq!(query.filters[0].op, Operator::Gte);
    }

    #[test]
    fn test_parse_error_invalid_query() {
        let result = parse_query("INVALID mood");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_select_star() {
        let query = parse_query("SELECT *").unwrap();
        assert_eq!(query.select.len(), 1);
        assert_eq!(query.select[0].metric, "*");
    }
}
