use crate::deps::sea_orm::sea_query::extension::postgres::PgExpr;
use crate::deps::sea_orm::sea_query::{Alias, Asterisk, Condition, Expr, JoinType, Query, SimpleExpr};
use crate::security::xss::Sanitizer;
use sea_orm::{DbBackend, Statement};

/// Simplified internal data structural representation of our custom search query
#[derive(Debug, Clone)]
pub struct CustomQuerySpec {
    pub base_table: String,
    pub select_columns: Vec<(Option<String>, String)>, // (Optional Table Prefix, Column Name)
    pub joins: Vec<JoinSpec>,
    pub r#where: Vec<WhereSpec>,
}

#[derive(Debug, Clone)]
pub struct JoinSpec {
    pub target_table: String,
    pub left_on: (String, String),  // (Table, Column)
    pub right_on: (String, String), // (Table, Column)
}

#[derive(Debug, Clone)]
pub struct WhereSpec {
    pub table: Option<String>,
    pub column: String,
    pub operator: String, // "=", "LIKE", ">"
    pub value: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct DynamicColumnSpec {
    pub name: String,
    pub r#type: String,
}

/// Dynamic compiler translating structural specs into database-agnostic statements
pub struct JqlCompiler;

impl JqlCompiler {
    pub fn compile(spec: &CustomQuerySpec, backend: DbBackend) -> Statement {
        let mut select = Query::select();

        let is_wildcard = spec.select_columns.is_empty() 
            || spec.select_columns.iter().any(|(_, col)| col == "*");

        if is_wildcard {
            select.column(Asterisk);
        } else {
            for (table_opt, col) in &spec.select_columns {
                if let Some(tbl) = table_opt {
                    select.column((Alias::new(tbl), Alias::new(col)));
                } else {
                    select.column((Alias::new(&spec.base_table), Alias::new(col)));
                }
            }
        }

        select.from(Alias::new(&spec.base_table));

        for join in &spec.joins {
            let left_expr = Expr::col((Alias::new(&join.left_on.0), Alias::new(&join.left_on.1)));
            let right_expr = Expr::col((Alias::new(&join.right_on.0), Alias::new(&join.right_on.1)));

            select.join(
                JoinType::InnerJoin,
                Alias::new(&join.target_table),
                left_expr.eq(right_expr),
            );
        }

        let mut conditions = Condition::all();
        for cond in &spec.r#where {
            let col_ref = if let Some(tbl) = &cond.table {
                Expr::col((Alias::new(tbl), Alias::new(&cond.column)))
            } else {
                Expr::col((Alias::new(&spec.base_table), Alias::new(&cond.column)))
            };

            // Cast the column to text so int/varchar/timestamp columns all
            // compare cleanly against string literals (e.g. `priority = 2`).
            let col_text = col_ref.clone().cast_as(Alias::new("text"));

            // For >/< bind numeric-looking literals as numbers so those
            // comparisons work numerically (col is NOT cast to text here).
            let numeric_val: Option<f64> = cond.value.parse::<f64>().ok();

let clause = match cond.operator.as_str() {
    // Compare on text side so text/numeric/timestamp columns all match.
    "=" => col_text.eq(Expr::val(cond.value.clone())),
    "LIKE" => {
        // Case-insensitive substring match. Postgres uses ILIKE; other backends
        // fall back to (case-sensitive) LIKE.
        let pattern = format!("%{}%", cond.value);
        if matches!(backend, DbBackend::Postgres) {
            col_text.ilike(pattern)
        } else {
            col_text.like(pattern)
        }
    }
    ">" => match numeric_val {
        Some(n) => col_ref.gt(Expr::val(n)),
        None => col_ref.gt(Expr::val(cond.value.clone())),
    },
    "<" => match numeric_val {
        Some(n) => col_ref.lt(Expr::val(n)),
        None => col_ref.lt(Expr::val(cond.value.clone())),
    },
    _ => col_text.eq(Expr::val(cond.value.clone())),
};
            conditions = conditions.add(clause);
        }

        select.cond_where(conditions);

        backend.build(&select)
    }
}

impl CustomQuerySpec {
    pub fn parse_from_str(input: &str) -> Result<Self, String> {
        let input = Sanitizer::url_decode(input).replace(";", "");
        // Keep original casing so string comparisons stay exact; lowercase only
        // the tokens used to locate structural keywords (SELECT/FROM/JOIN/WHERE)
        let normalized = input.replace(",", " ");
        let tokens: Vec<&str> = normalized.split_whitespace().collect();
        let lc: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();

        // Detect core indexing components
        let select_idx = lc.iter().position(|t| t == "select");
        let from_idx = lc.iter().position(|t| t == "from");
        let join_idx = lc.iter().position(|t| t == "join");
        let where_idx = lc.iter().position(|t| t == "where");

        if select_idx.is_none() || from_idx.is_none() {
            return Err(
                "Invalid structural syntax: Queries must include SELECT and FROM clauses."
                    .to_string(),
            );
        }

        let from_table = tokens[from_idx.unwrap() + 1].to_string();
        let mut select_columns = Vec::new();

        // Parse target columns
        for i in (select_idx.unwrap() + 1)..from_idx.unwrap() {
            let part = tokens[i];
            if part.contains('.') {
                let chunks: Vec<&str> = part.split('.').collect();
                select_columns.push((Some(chunks[0].to_string()), chunks[1].to_string()));
            } else {
                select_columns.push((None, part.to_string()));
            }
        }

        // Extract dynamic joins if present
        let mut joins = Vec::new();
        if let Some(j_idx) = join_idx {
            let on_idx = lc.iter().position(|t| t == "on");
            if let Some(o_idx) = on_idx {
                let target_table = tokens[j_idx + 1].to_string();
                let left_side = tokens[o_idx + 1].split('.').collect::<Vec<&str>>();
                let right_side = tokens[o_idx + 3].split('.').collect::<Vec<&str>>();

                joins.push(JoinSpec {
                    target_table,
                    left_on: (left_side[0].to_string(), left_side[1].to_string()),
                    right_on: (right_side[0].to_string(), right_side[1].to_string()),
                });
            }
        }

        // Extract where condition targets
        let mut conditions = Vec::new();
        if let Some(w_idx) = where_idx {
            let col_part = tokens[w_idx + 1];
            let operator = tokens[w_idx + 2].to_uppercase();
            let value = tokens[w_idx + 3].replace("'", "");

            if col_part.contains('.') {
                let chunks: Vec<&str> = col_part.split('.').collect();
                conditions.push(WhereSpec {
                    table: Some(chunks[0].to_string()),
                    column: chunks[1].to_string(),
                    operator,
                    value,
                });
            } else {
                conditions.push(WhereSpec {
                    table: None,
                    column: col_part.to_string(),
                    operator,
                    value,
                });
            }
        }

        Ok(CustomQuerySpec {
            base_table: from_table,
            select_columns,
            joins,
            r#where: conditions,
        })
    }
}
