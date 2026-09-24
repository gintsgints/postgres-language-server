use std::error::Error;

use super::TRIVIA_TOKENS;
use pgls_lexer::SyntaxKind;

use super::{
    Splitter,
    data::at_statement_start,
    ddl::{alter, create},
    dml::{cte, delete, explain, insert, select, update},
};

#[derive(Debug)]
pub struct ReachedEOFException;

impl std::fmt::Display for ReachedEOFException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReachedEOFException")
    }
}

impl Error for ReachedEOFException {}

/// Tokens that cannot end a statement: when one sits in front of a blank line,
/// the statement is unfinished and continues after it, e.g. `select * from\n\ncustomers`.
/// Every [`CONTINUATION_TOKENS`] keyword counts as unfinished too - see
/// [`cannot_end_statement`] - which covers `select 1\n\nunion\n\nselect 2`,
/// where the blank line precedes a keyword that could otherwise start a
/// statement of its own.
static UNFINISHED_TOKENS: &[SyntaxKind] = &[
    SyntaxKind::COMMA,
    SyntaxKind::ALL_KW,
    SyntaxKind::AS_KW,
    SyntaxKind::BY_KW,
    SyntaxKind::SET_KW,
    SyntaxKind::INTO_KW,
    SyntaxKind::VALUES_KW,
    SyntaxKind::DISTINCT_KW,
    SyntaxKind::NOT_KW,
    SyntaxKind::IS_KW,
    SyntaxKind::IN_KW,
    SyntaxKind::LIKE_KW,
    SyntaxKind::ILIKE_KW,
    SyntaxKind::SIMILAR_KW,
    SyntaxKind::BETWEEN_KW,
    // operators are always waiting for a right-hand operand
    SyntaxKind::EQ,
    SyntaxKind::BANG,
    SyntaxKind::L_ANGLE,
    SyntaxKind::R_ANGLE,
    SyntaxKind::PLUS,
    SyntaxKind::MINUS,
    SyntaxKind::SLASH,
    SyntaxKind::PERCENT,
    SyntaxKind::CARET,
    SyntaxKind::AMP,
    SyntaxKind::PIPE,
    SyntaxKind::TILDE,
    SyntaxKind::AT,
    SyntaxKind::COLON,
    SyntaxKind::DOUBLE_COLON,
    SyntaxKind::DOT,
];

/// Keywords that can only continue a statement, never start one. A blank line
/// in front of one of these is formatting, not a statement boundary, so the
/// current statement keeps going instead of being cut in two.
static CONTINUATION_TOKENS: &[SyntaxKind] = &[
    SyntaxKind::FROM_KW,
    SyntaxKind::WHERE_KW,
    SyntaxKind::JOIN_KW,
    SyntaxKind::INNER_KW,
    SyntaxKind::LEFT_KW,
    SyntaxKind::RIGHT_KW,
    SyntaxKind::FULL_KW,
    SyntaxKind::CROSS_KW,
    SyntaxKind::NATURAL_KW,
    SyntaxKind::LATERAL_KW,
    SyntaxKind::ON_KW,
    SyntaxKind::USING_KW,
    SyntaxKind::GROUP_KW,
    SyntaxKind::HAVING_KW,
    SyntaxKind::WINDOW_KW,
    SyntaxKind::ORDER_KW,
    SyntaxKind::LIMIT_KW,
    SyntaxKind::OFFSET_KW,
    SyntaxKind::FETCH_KW,
    SyntaxKind::RETURNING_KW,
    SyntaxKind::UNION_KW,
    SyntaxKind::INTERSECT_KW,
    SyntaxKind::EXCEPT_KW,
    SyntaxKind::AND_KW,
    SyntaxKind::OR_KW,
];

/// Whether `kind` sitting right before a blank line leaves the statement
/// unfinished. A clause keyword cannot start a statement, so it cannot end one
/// either: `select * from\n\ncustomers` is one statement, not two.
fn cannot_end_statement(kind: SyntaxKind) -> bool {
    UNFINISHED_TOKENS.contains(&kind) || CONTINUATION_TOKENS.contains(&kind)
}

pub(crate) type SplitterResult = std::result::Result<(), ReachedEOFException>;

pub fn source(p: &mut Splitter) -> SplitterResult {
    loop {
        match p.current() {
            SyntaxKind::EOF => {
                break;
            }
            kind if TRIVIA_TOKENS.contains(&kind) || kind == SyntaxKind::LINE_ENDING => {
                p.advance()?;
            }
            SyntaxKind::BACKSLASH => {
                plpgsql_command(p)?;
            }
            _ => {
                statement(p)?;
            }
        }
    }

    Ok(())
}

pub(crate) fn statement(p: &mut Splitter) -> SplitterResult {
    p.start_stmt();

    // Currently, Err means that we reached EOF.
    // Regardless of whether we reach EOF or we complete the statement, we want to close it.
    // We might want to handle other kinds of errors differently in the future.
    let _ = match p.current() {
        SyntaxKind::WITH_KW => cte(p),
        SyntaxKind::SELECT_KW => select(p),
        SyntaxKind::INSERT_KW => insert(p),
        SyntaxKind::UPDATE_KW => update(p),
        SyntaxKind::DELETE_KW => delete(p),
        SyntaxKind::CREATE_KW => create(p),
        SyntaxKind::ALTER_KW => alter(p),
        SyntaxKind::EXPLAIN_KW => explain(p),
        _ => unknown(p, &[]),
    };

    p.close_stmt();

    Ok(())
}

pub(crate) fn begin_end(p: &mut Splitter) -> SplitterResult {
    p.expect(SyntaxKind::BEGIN_KW)?;

    let mut depth = 1;

    loop {
        match p.current() {
            SyntaxKind::BEGIN_KW => {
                p.advance()?;
                depth += 1;
            }
            SyntaxKind::END_KW => {
                if p.current() == SyntaxKind::END_KW {
                    p.advance()?;
                }
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {
                p.advance()?;
            }
        }
    }

    Ok(())
}

pub(crate) fn parenthesis(p: &mut Splitter) -> SplitterResult {
    p.expect(SyntaxKind::L_PAREN)?;

    let mut depth = 1;

    loop {
        match p.current() {
            SyntaxKind::L_PAREN => {
                p.advance()?;
                depth += 1;
            }
            SyntaxKind::R_PAREN => {
                if p.current() == SyntaxKind::R_PAREN {
                    p.advance()?;
                }
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {
                p.advance()?;
            }
        }
    }

    Ok(())
}

pub(crate) fn plpgsql_command(p: &mut Splitter) -> SplitterResult {
    p.expect(SyntaxKind::BACKSLASH)?;

    loop {
        match p.current() {
            SyntaxKind::LINE_ENDING => {
                p.advance()?;
                break;
            }
            _ => {
                // advance the splitter to the next token without ignoring irrelevant tokens
                // we would skip a newline with `advance()`
                p.step()?;
            }
        }
    }

    Ok(())
}

pub(crate) fn case(p: &mut Splitter) -> SplitterResult {
    p.expect(SyntaxKind::CASE_KW)?;

    loop {
        match p.current() {
            SyntaxKind::END_KW => {
                p.advance()?;
                break;
            }
            _ => {
                p.advance()?;
            }
        }
    }

    Ok(())
}

pub(crate) fn unknown(p: &mut Splitter, exclude: &[SyntaxKind]) -> SplitterResult {
    loop {
        match p.current() {
            SyntaxKind::SEMICOLON => {
                p.advance()?;
                break;
            }
            SyntaxKind::LINE_ENDING => {
                if p.look_back(true).is_some_and(cannot_end_statement)
                    || CONTINUATION_TOKENS.contains(&p.look_ahead(true))
                {
                    p.advance()?;
                } else {
                    break;
                }
            }
            SyntaxKind::CASE_KW => {
                case(p)?;
            }
            SyntaxKind::BACKSLASH => {
                // pgsql commands
                // we want to check if the previous token non-trivia token is a LINE_ENDING
                // we cannot use the is_trivia() method because that would exclude LINE_ENDINGs
                // with count > 1
                if (0..p.current_pos)
                    .rev()
                    .find_map(|idx| {
                        let kind = p.kind(idx);
                        if !TRIVIA_TOKENS.contains(&kind) {
                            Some(kind)
                        } else {
                            None
                        }
                    })
                    .is_some_and(|t| t == SyntaxKind::LINE_ENDING)
                {
                    break;
                }
                p.advance()?;
            }
            SyntaxKind::L_PAREN => {
                parenthesis(p)?;
            }
            SyntaxKind::BEGIN_KW => match p.look_ahead(true) {
                SyntaxKind::SEMICOLON => {
                    p.advance()?;
                }
                SyntaxKind::ATOMIC_KW => {
                    begin_end(p)?;
                }
                SyntaxKind::TRANSACTION_KW
                | SyntaxKind::WORK_KW
                | SyntaxKind::ISOLATION_KW
                | SyntaxKind::READ_KW
                | SyntaxKind::WRITE_KW
                | SyntaxKind::ONLY_KW
                | SyntaxKind::DEFERRABLE_KW
                | SyntaxKind::NOT_KW => {
                    p.advance()?;
                }
                _ => {
                    begin_end(p)?;
                }
            },
            t => match at_statement_start(t, exclude) {
                Some(SyntaxKind::SELECT_KW) => {
                    let prev = p.look_back_across_blank_lines();
                    if [
                        // for policies, with for select
                        SyntaxKind::FOR_KW,
                        // for create view / table as
                        SyntaxKind::AS_KW,
                        // for create rule
                        SyntaxKind::ON_KW,
                        // for create rule
                        SyntaxKind::ALSO_KW,
                        // for create rule
                        SyntaxKind::INSTEAD_KW,
                        // for UNION
                        SyntaxKind::UNION_KW,
                        // for UNION ALL
                        SyntaxKind::ALL_KW,
                        // for UNION ... EXCEPT
                        SyntaxKind::EXCEPT_KW,
                        // for grant
                        SyntaxKind::GRANT_KW,
                        // for revoke
                        SyntaxKind::REVOKE_KW,
                        SyntaxKind::COMMA,
                        // for BEGIN ATOMIC
                        SyntaxKind::ATOMIC_KW,
                    ]
                    .iter()
                    .all(|x| Some(x) != prev.as_ref())
                    {
                        break;
                    }

                    p.advance()?;
                }
                // DELETE is also an unreserved keyword used by hstore's delete() function.
                // A DELETE statement must be followed by FROM, so `delete(` cannot start one.
                Some(SyntaxKind::DELETE_KW) if p.look_ahead(true) == SyntaxKind::L_PAREN => {
                    p.advance()?;
                }
                Some(SyntaxKind::INSERT_KW)
                | Some(SyntaxKind::UPDATE_KW)
                | Some(SyntaxKind::DELETE_KW) => {
                    let prev = p.look_back_across_blank_lines();
                    if [
                        // for create trigger
                        SyntaxKind::BEFORE_KW,
                        SyntaxKind::AFTER_KW,
                        // for policies, e.g. for insert
                        SyntaxKind::FOR_KW,
                        // e.g. on insert or delete
                        SyntaxKind::OR_KW,
                        // e.g. INSTEAD OF INSERT
                        SyntaxKind::OF_KW,
                        // for create rule
                        SyntaxKind::ON_KW,
                        // for create rule
                        SyntaxKind::ALSO_KW,
                        // for create rule
                        SyntaxKind::INSTEAD_KW,
                        // for grant
                        SyntaxKind::GRANT_KW,
                        // for revoke
                        SyntaxKind::REVOKE_KW,
                        SyntaxKind::COMMA,
                        // Do update in INSERT stmt
                        SyntaxKind::DO_KW,
                        // FOR NO KEY UPDATE
                        SyntaxKind::KEY_KW,
                        // WHEN MATCHED THEN
                        SyntaxKind::THEN_KW,
                    ]
                    .iter()
                    .all(|x| Some(x) != prev.as_ref())
                    {
                        break;
                    }
                    p.advance()?;
                }
                Some(SyntaxKind::WITH_KW) => {
                    let next = p.look_ahead(true);
                    if [
                        // WITH ORDINALITY should not start a new statement
                        SyntaxKind::ORDINALITY_KW,
                        // WITH CHECK should not start a new statement
                        SyntaxKind::CHECK_KW,
                        // TIMESTAMP WITH TIME ZONE should not start a new statement
                        SyntaxKind::TIME_KW,
                        SyntaxKind::GRANT_KW,
                        SyntaxKind::ADMIN_KW,
                        SyntaxKind::INHERIT_KW,
                        SyntaxKind::SET_KW,
                    ]
                    .iter()
                    .all(|x| x != &next)
                    {
                        break;
                    }
                    p.advance()?;
                }
                Some(SyntaxKind::CREATE_KW) => {
                    let prev = p.look_back_across_blank_lines();
                    if [
                        // for grant
                        SyntaxKind::GRANT_KW,
                        // for revoke
                        SyntaxKind::REVOKE_KW,
                        SyntaxKind::COMMA,
                    ]
                    .iter()
                    .all(|x| Some(x) != prev.as_ref())
                    {
                        break;
                    }

                    p.advance()?;
                }
                Some(_) => {
                    break;
                }
                None => {
                    p.advance()?;
                }
            },
        }
    }
    Ok(())
}
