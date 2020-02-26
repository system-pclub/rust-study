// Copyright 2017 PingCAP, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

use std::sync::Arc;

use tipb::executor::Selection;

use crate::coprocessor::dag::expr::{EvalConfig, EvalContext, EvalWarnings, Expression};
use crate::coprocessor::Result;

use super::{Executor, ExecutorMetrics, ExprColumnRefVisitor, Row};

/// Retrieves rows from the source executor and filter rows by expressions.
pub struct SelectionExecutor {
    conditions: Vec<Expression>,
    related_cols_offset: Vec<usize>, // offset of related columns
    ctx: EvalContext,
    src: Box<dyn Executor + Send>,
    first_collect: bool,
}

impl SelectionExecutor {
    pub fn new(
        mut meta: Selection,
        eval_cfg: Arc<EvalConfig>,
        src: Box<dyn Executor + Send>,
    ) -> Result<SelectionExecutor> {
        let conditions = meta.take_conditions().into_vec();
        let mut visitor = ExprColumnRefVisitor::new(src.get_len_of_columns());
        visitor.batch_visit(&conditions)?;
        let ctx = EvalContext::new(eval_cfg);
        Ok(SelectionExecutor {
            conditions: Expression::batch_build(&ctx, conditions)?,
            related_cols_offset: visitor.column_offsets(),
            ctx,
            src,
            first_collect: true,
        })
    }
}

impl Executor for SelectionExecutor {
    fn next(&mut self) -> Result<Option<Row>> {
        'next: while let Some(row) = self.src.next()? {
            let row = row.take_origin();
            let cols = row.inflate_cols_with_offsets(&self.ctx, &self.related_cols_offset)?;
            for filter in &self.conditions {
                let val = filter.eval(&mut self.ctx, &cols)?;
                if !val.into_bool(&mut self.ctx)?.unwrap_or(false) {
                    continue 'next;
                }
            }
            return Ok(Some(Row::Origin(row)));
        }
        Ok(None)
    }

    fn collect_output_counts(&mut self, counts: &mut Vec<i64>) {
        self.src.collect_output_counts(counts);
    }

    fn collect_metrics_into(&mut self, metrics: &mut ExecutorMetrics) {
        self.src.collect_metrics_into(metrics);
        if self.first_collect {
            metrics.executor_count.selection += 1;
            self.first_collect = false;
        }
    }

    fn take_eval_warnings(&mut self) -> Option<EvalWarnings> {
        if let Some(mut warnings) = self.src.take_eval_warnings() {
            warnings.merge(&mut self.ctx.take_warnings());
            Some(warnings)
        } else {
            Some(self.ctx.take_warnings())
        }
    }

    fn get_len_of_columns(&self) -> usize {
        self.src.get_len_of_columns()
    }
}

#[cfg(test)]
mod tests {
    use std::i64;
    use std::sync::Arc;

    use cop_datatype::FieldTypeTp;
    use tipb::expression::{Expr, ExprType, ScalarFuncSig};

    use crate::coprocessor::codec::datum::Datum;
    use crate::util::codec::number::NumberEncoder;

    use super::super::tests::{gen_table_scan_executor, new_col_info};
    use super::*;

    fn new_const_expr() -> Expr {
        let mut expr = Expr::new();
        expr.set_tp(ExprType::ScalarFunc);
        expr.set_sig(ScalarFuncSig::NullEQInt);
        expr.mut_children().push({
            let mut lhs = Expr::new();
            lhs.set_tp(ExprType::Null);
            lhs
        });
        expr.mut_children().push({
            let mut rhs = Expr::new();
            rhs.set_tp(ExprType::Null);
            rhs
        });
        expr
    }

    fn new_col_gt_u64_expr(offset: i64, val: u64) -> Expr {
        let mut expr = Expr::new();
        expr.set_tp(ExprType::ScalarFunc);
        expr.set_sig(ScalarFuncSig::GTInt);
        expr.mut_children().push({
            let mut lhs = Expr::new();
            lhs.set_tp(ExprType::ColumnRef);
            lhs.mut_val().encode_i64(offset).unwrap();
            lhs
        });
        expr.mut_children().push({
            let mut rhs = Expr::new();
            rhs.set_tp(ExprType::Uint64);
            rhs.mut_val().encode_u64(val).unwrap();
            rhs
        });
        expr
    }

    #[test]
    fn test_selection_executor_simple() {
        let cis = vec![
            new_col_info(1, FieldTypeTp::LongLong),
            new_col_info(2, FieldTypeTp::VarChar),
            new_col_info(3, FieldTypeTp::NewDecimal),
        ];
        let raw_data = vec![
            vec![
                Datum::I64(1),
                Datum::Bytes(b"a".to_vec()),
                Datum::Dec(7.into()),
            ],
            vec![
                Datum::I64(2),
                Datum::Bytes(b"b".to_vec()),
                Datum::Dec(7.into()),
            ],
            vec![
                Datum::I64(3),
                Datum::Bytes(b"b".to_vec()),
                Datum::Dec(8.into()),
            ],
            vec![
                Datum::I64(4),
                Datum::Bytes(b"d".to_vec()),
                Datum::Dec(3.into()),
            ],
            vec![
                Datum::I64(5),
                Datum::Bytes(b"f".to_vec()),
                Datum::Dec(5.into()),
            ],
            vec![
                Datum::I64(6),
                Datum::Bytes(b"e".to_vec()),
                Datum::Dec(9.into()),
            ],
            vec![
                Datum::I64(7),
                Datum::Bytes(b"f".to_vec()),
                Datum::Dec(6.into()),
            ],
        ];

        let inner_table_scan = gen_table_scan_executor(1, cis, &raw_data, None);

        // selection executor
        let mut selection = Selection::new();
        let expr = new_const_expr();
        selection.mut_conditions().push(expr);

        let mut selection_executor =
            SelectionExecutor::new(selection, Arc::new(EvalConfig::default()), inner_table_scan)
                .unwrap();

        let mut selection_rows = Vec::with_capacity(raw_data.len());
        while let Some(row) = selection_executor.next().unwrap() {
            selection_rows.push(row.take_origin());
        }

        assert_eq!(selection_rows.len(), raw_data.len());
        let expect_row_handles = raw_data.iter().map(|r| r[0].i64()).collect::<Vec<_>>();
        let result_row = selection_rows.iter().map(|r| r.handle).collect::<Vec<_>>();
        assert_eq!(result_row, expect_row_handles);
    }

    #[test]
    fn test_selection_executor_condition() {
        let cis = vec![
            new_col_info(1, FieldTypeTp::LongLong),
            new_col_info(2, FieldTypeTp::VarChar),
            new_col_info(3, FieldTypeTp::LongLong),
        ];
        let raw_data = vec![
            vec![Datum::I64(1), Datum::Bytes(b"a".to_vec()), Datum::I64(7)],
            vec![Datum::I64(2), Datum::Bytes(b"b".to_vec()), Datum::I64(7)],
            vec![Datum::I64(3), Datum::Bytes(b"b".to_vec()), Datum::I64(8)],
            vec![Datum::I64(4), Datum::Bytes(b"d".to_vec()), Datum::I64(3)],
            vec![Datum::I64(5), Datum::Bytes(b"f".to_vec()), Datum::I64(5)],
            vec![Datum::I64(6), Datum::Bytes(b"e".to_vec()), Datum::I64(9)],
            vec![Datum::I64(7), Datum::Bytes(b"f".to_vec()), Datum::I64(6)],
        ];

        let inner_table_scan = gen_table_scan_executor(1, cis, &raw_data, None);

        // selection executor
        let mut selection = Selection::new();
        let expr = new_col_gt_u64_expr(2, 5);
        selection.mut_conditions().push(expr);

        let mut selection_executor =
            SelectionExecutor::new(selection, Arc::new(EvalConfig::default()), inner_table_scan)
                .unwrap();

        let mut selection_rows = Vec::with_capacity(raw_data.len());
        while let Some(row) = selection_executor.next().unwrap() {
            selection_rows.push(row.take_origin());
        }

        let expect_row_handles = raw_data
            .iter()
            .filter(|r| r[2].i64() > 5)
            .map(|r| r[0].i64())
            .collect::<Vec<_>>();
        assert!(expect_row_handles.len() < raw_data.len());
        assert_eq!(selection_rows.len(), expect_row_handles.len());
        let result_row = selection_rows.iter().map(|r| r.handle).collect::<Vec<_>>();
        assert_eq!(result_row, expect_row_handles);
        let expected_counts = vec![raw_data.len() as i64];
        let mut counts = Vec::with_capacity(1);
        selection_executor.collect_output_counts(&mut counts);
        assert_eq!(expected_counts, counts);
    }
}
