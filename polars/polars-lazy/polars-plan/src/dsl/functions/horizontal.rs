use super::*;

#[cfg(feature = "dtype-struct")]
fn cumfold_dtype() -> GetOutput {
    GetOutput::map_fields(|fields| {
        let mut st = fields[0].dtype.clone();
        for fld in &fields[1..] {
            st = get_supertype(&st, &fld.dtype).unwrap();
        }
        Field::new(
            &fields[0].name,
            DataType::Struct(
                fields
                    .iter()
                    .map(|fld| Field::new(fld.name(), st.clone()))
                    .collect(),
            ),
        )
    })
}

/// Accumulate over multiple columns horizontally / row wise.
pub fn fold_exprs<F: 'static, E: AsRef<[Expr]>>(acc: Expr, f: F, exprs: E) -> Expr
where
    F: Fn(Series, Series) -> PolarsResult<Option<Series>> + Send + Sync + Clone,
{
    let mut exprs = exprs.as_ref().to_vec();
    exprs.push(acc);

    let function = SpecialEq::new(Arc::new(move |series: &mut [Series]| {
        let mut series = series.to_vec();
        let mut acc = series.pop().unwrap();

        for s in series {
            if let Some(a) = f(acc.clone(), s)? {
                acc = a
            }
        }
        Ok(Some(acc))
    }) as Arc<dyn SeriesUdf>);

    Expr::AnonymousFunction {
        input: exprs,
        function,
        output_type: GetOutput::super_type(),
        options: FunctionOptions {
            collect_groups: ApplyOptions::ApplyGroups,
            input_wildcard_expansion: true,
            auto_explode: true,
            fmt_str: "fold",
            ..Default::default()
        },
    }
}

/// Analogous to [`Iterator::reduce`](std::iter::Iterator::reduce).
///
/// An accumulator is initialized to the series given by the first expression in `exprs`, and then each subsequent value
/// of the accumulator is computed from `f(acc, next_expr_series)`. If `exprs` is empty, an error is returned when
/// `collect` is called.
pub fn reduce_exprs<F: 'static, E: AsRef<[Expr]>>(f: F, exprs: E) -> Expr
where
    F: Fn(Series, Series) -> PolarsResult<Option<Series>> + Send + Sync + Clone,
{
    let exprs = exprs.as_ref().to_vec();

    let function = SpecialEq::new(Arc::new(move |series: &mut [Series]| {
        let mut s_iter = series.iter();

        match s_iter.next() {
            Some(acc) => {
                let mut acc = acc.clone();

                for s in s_iter {
                    if let Some(a) = f(acc.clone(), s.clone())? {
                        acc = a
                    }
                }
                Ok(Some(acc))
            }
            None => Err(polars_err!(ComputeError: "`reduce` did not have any expressions to fold")),
        }
    }) as Arc<dyn SeriesUdf>);

    Expr::AnonymousFunction {
        input: exprs,
        function,
        output_type: GetOutput::super_type(),
        options: FunctionOptions {
            collect_groups: ApplyOptions::ApplyGroups,
            input_wildcard_expansion: true,
            auto_explode: true,
            fmt_str: "reduce",
            ..Default::default()
        },
    }
}

/// Accumulate over multiple columns horizontally / row wise.
#[cfg(feature = "dtype-struct")]
pub fn cumreduce_exprs<F: 'static, E: AsRef<[Expr]>>(f: F, exprs: E) -> Expr
where
    F: Fn(Series, Series) -> PolarsResult<Option<Series>> + Send + Sync + Clone,
{
    let exprs = exprs.as_ref().to_vec();

    let function = SpecialEq::new(Arc::new(move |series: &mut [Series]| {
        let mut s_iter = series.iter();

        match s_iter.next() {
            Some(acc) => {
                let mut acc = acc.clone();
                let mut result = vec![acc.clone()];

                for s in s_iter {
                    let name = s.name().to_string();
                    if let Some(a) = f(acc.clone(), s.clone())? {
                        acc = a;
                    }
                    acc.rename(&name);
                    result.push(acc.clone());
                }

                StructChunked::new(acc.name(), &result).map(|ca| Some(ca.into_series()))
            }
            None => Err(polars_err!(ComputeError: "`reduce` did not have any expressions to fold")),
        }
    }) as Arc<dyn SeriesUdf>);

    Expr::AnonymousFunction {
        input: exprs,
        function,
        output_type: cumfold_dtype(),
        options: FunctionOptions {
            collect_groups: ApplyOptions::ApplyGroups,
            input_wildcard_expansion: true,
            auto_explode: true,
            fmt_str: "cumreduce",
            ..Default::default()
        },
    }
}

/// Accumulate over multiple columns horizontally / row wise.
#[cfg(feature = "dtype-struct")]
pub fn cumfold_exprs<F: 'static, E: AsRef<[Expr]>>(
    acc: Expr,
    f: F,
    exprs: E,
    include_init: bool,
) -> Expr
where
    F: Fn(Series, Series) -> PolarsResult<Option<Series>> + Send + Sync + Clone,
{
    let mut exprs = exprs.as_ref().to_vec();
    exprs.push(acc);

    let function = SpecialEq::new(Arc::new(move |series: &mut [Series]| {
        let mut series = series.to_vec();
        let mut acc = series.pop().unwrap();

        let mut result = vec![];
        if include_init {
            result.push(acc.clone())
        }

        for s in series {
            let name = s.name().to_string();
            if let Some(a) = f(acc.clone(), s)? {
                acc = a;
                acc.rename(&name);
                result.push(acc.clone());
            }
        }

        StructChunked::new(acc.name(), &result).map(|ca| Some(ca.into_series()))
    }) as Arc<dyn SeriesUdf>);

    Expr::AnonymousFunction {
        input: exprs,
        function,
        output_type: cumfold_dtype(),
        options: FunctionOptions {
            collect_groups: ApplyOptions::ApplyGroups,
            input_wildcard_expansion: true,
            auto_explode: true,
            fmt_str: "cumfold",
            ..Default::default()
        },
    }
}

/// Create a new column with the the sum of the values in each row.
///
/// The name of the resulting column will be `"sum"`; use [`alias`](Expr::alias) to choose a different name.
pub fn sum_exprs<E: AsRef<[Expr]>>(exprs: E) -> Expr {
    let mut exprs = exprs.as_ref().to_vec();
    let func = |s1, s2| Ok(Some(&s1 + &s2));
    let init = match exprs.pop() {
        Some(e) => e,
        // use u32 as that is not cast to float as eagerly
        _ => lit(0u32),
    };
    fold_exprs(init, func, exprs).alias("sum")
}

/// Create a new column with the the maximum value per row.
///
/// The name of the resulting column will be `"max"`; use [`alias`](Expr::alias) to choose a different name.
pub fn max_exprs<E: AsRef<[Expr]>>(exprs: E) -> Expr {
    let exprs = exprs.as_ref().to_vec();
    if exprs.is_empty() {
        return Expr::Columns(Vec::new());
    }
    let func = |s1, s2| {
        let df = DataFrame::new_no_checks(vec![s1, s2]);
        df.hmax()
    };
    reduce_exprs(func, exprs).alias("max")
}

/// Create a new column with the the minimum value per row.
///
/// The name of the resulting column will be `"min"`; use [`alias`](Expr::alias) to choose a different name.
pub fn min_exprs<E: AsRef<[Expr]>>(exprs: E) -> Expr {
    let exprs = exprs.as_ref().to_vec();
    if exprs.is_empty() {
        return Expr::Columns(Vec::new());
    }
    let func = |s1, s2| {
        let df = DataFrame::new_no_checks(vec![s1, s2]);
        df.hmin()
    };
    reduce_exprs(func, exprs).alias("min")
}

/// Create a new column with the the bitwise-or of the elements in each row.
///
/// The name of the resulting column is arbitrary; use [`alias`](Expr::alias) to choose a different name.
pub fn any_exprs<E: AsRef<[Expr]>>(exprs: E) -> Expr {
    let exprs = exprs.as_ref().to_vec();
    let func = |s1: Series, s2: Series| Ok(Some(s1.bool()?.bitor(s2.bool()?).into_series()));
    fold_exprs(lit(false), func, exprs)
}

/// Create a new column with the the bitwise-and of the elements in each row.
///
/// The name of the resulting column is arbitrary; use [`alias`](Expr::alias) to choose a different name.
pub fn all_exprs<E: AsRef<[Expr]>>(exprs: E) -> Expr {
    let exprs = exprs.as_ref().to_vec();
    let func = |s1: Series, s2: Series| Ok(Some(s1.bool()?.bitand(s2.bool()?).into_series()));
    fold_exprs(lit(true), func, exprs)
}

/// Folds the expressions from left to right keeping the first non-null values.
///
/// It is an error to provide an empty `exprs`.
pub fn coalesce(exprs: &[Expr]) -> Expr {
    let input = exprs.to_vec();
    Expr::Function {
        input,
        function: FunctionExpr::Coalesce,
        options: FunctionOptions {
            collect_groups: ApplyOptions::ApplyGroups,
            cast_to_supertypes: true,
            input_wildcard_expansion: true,
            ..Default::default()
        },
    }
}
