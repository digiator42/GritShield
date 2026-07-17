use crate::deps::sea_orm::sea_query::{Alias, Condition, Expr, JoinType, Query};
use crate::security::xss::Sanitizer;
use sea_orm::{ DbBackend, Statement};

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

        // Process explicit select targets or fallback safely to wildcard definitions
        if spec.select_columns.is_empty() {
            select.column((Alias::new(&spec.base_table), Alias::new("*")));
        } else {
            for (table_opt, col) in &spec.select_columns {
                if let Some(tbl) = table_opt {
                    select.column((Alias::new(tbl), Alias::new(col)));
                } else {
                    select.column((Alias::new(&spec.base_table), Alias::new(col)));
                }
            }
        }

        // Define root origin table target matrix
        select.from(Alias::new(&spec.base_table));

        // Append relational joins dynamically
        for join in &spec.joins {
            let left_expr = Expr::col((Alias::new(&join.left_on.0), Alias::new(&join.left_on.1)));
            let right_expr =
                Expr::col((Alias::new(&join.right_on.0), Alias::new(&join.right_on.1)));

            // Cleanly link the two column expression definitions together
            select.join(
                JoinType::InnerJoin,
                Alias::new(&join.target_table),
                left_expr.eq(right_expr),
            );
        }

        // Inject runtime query condition filters safely
        let mut conditions = Condition::all();
        for cond in &spec.r#where {
            let col_ref = if let Some(tbl) = &cond.table {
                Expr::col((Alias::new(tbl), Alias::new(&cond.column)))
            } else {
                Expr::col((Alias::new(&spec.base_table), Alias::new(&cond.column)))
            };

            // Fix: Wrap raw strings inside Expr::val to yield structural parameters
            let clause = match cond.operator.as_str() {
                "=" => col_ref.eq(Expr::val(cond.value.clone())),
                "LIKE" => col_ref.like(format!("%{}%", cond.value)),
                ">" => col_ref.gt(Expr::val(cond.value.clone())),
                "<" => col_ref.lt(Expr::val(cond.value.clone())),
                _ => col_ref.eq(Expr::val(cond.value.clone())),
            };
            conditions = conditions.add(clause);
        }

        select.cond_where(conditions);

        // Generate target-compiled SQL variant safely
        backend.build(&select)
    }
}

impl CustomQuerySpec {
    pub fn parse_from_str(input: &str) -> Result<Self, String> {
        let input = Sanitizer::url_decode(input).replace(";", "");
        let normalized = input.replace(",", " ").to_lowercase();
        let tokens: Vec<&str> = normalized.split_whitespace().collect();

        // Detect core indexing components
        let select_idx = tokens.iter().position(|&t| t == "select");
        let from_idx = tokens.iter().position(|&t| t == "from");
        let join_idx = tokens.iter().position(|&t| t == "join");
        let where_idx = tokens.iter().position(|&t| t == "where");

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
            let on_idx = tokens.iter().position(|&t| t == "on");
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
            let operator = tokens[w_idx + 2].to_string();
            let value = tokens[w_idx + 3].replace("'", "").to_string();

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
