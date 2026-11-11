//! QueryBuilder 编译矩阵特征化测试（计划 Task1 / Review Focus 钉子）。
//! 纯函数无 DB。规则来源：Spike V-1 结论（等值/IN 走 @> GIN；like/range/排序拒绝未提升列）。

use core_model::query::{CompiledQuery, ObjectQuery, Op, Page, QueryBuilder};
use core_model::{AttrDef, BaseType, DataType, TypeDef};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

fn attr(code: &str, dt: DataType, promoted: bool) -> AttrDef {
    AttrDef {
        code: code.into(),
        name: code.into(),
        data_type: dt,
        required: false,
        unique_flag: false,
        default_val: None,
        ref_type: None,
        formula: None,
        promoted,
        promoted_col: promoted.then(|| format!("p_{code}")),
        config: json!({}),
    }
}

fn td() -> Arc<TypeDef> {
    let attrs = vec![
        attr("material", DataType::Text, true),
        attr("remark", DataType::Text, false),
        attr("weight", DataType::Decimal, true),
    ];
    let attr_index = attrs
        .iter()
        .enumerate()
        .map(|(i, a)| (a.code.clone(), i))
        .collect();
    Arc::new(TypeDef {
        id: Uuid::nil(),
        code: "drawing".into(),
        name: "图纸".into(),
        base_type: BaseType::Document,
        numbering_rule: None,
        title_attr: None,
        config: json!({}),
        config_version: 1,
        attrs,
        attr_index,
    })
}

fn compile(q: &ObjectQuery) -> CompiledQuery {
    QueryBuilder::compile(td(), q).expect("compile ok")
}

fn one(attr: &str, op: Op, value: Value) -> ObjectQuery {
    ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![core_model::query::Cond {
            attr: attr.into(),
            op,
            value,
        }],
        sorts: vec![],
        page: None,
    }
}

#[test]
fn eq_promoted_column_uses_physical_col() {
    let c = compile(&one("material", Op::Eq, json!("Q235")));
    assert!(
        c.where_sql.contains("p_material = $1::text"),
        "got: {}",
        c.where_sql
    );
    assert_eq!(c.binds[0].value, json!("Q235"));
}

#[test]
fn eq_unpromoted_forces_containment() {
    let c = compile(&one("remark", Op::Eq, json!("hello")));
    assert!(
        c.where_sql.contains("attrs @> $1::jsonb"),
        "got: {}",
        c.where_sql
    );
    assert_eq!(
        c.binds[0].value,
        json!({"remark": "hello"}),
        "bind 必须是 containment 对象"
    );
}

#[test]
fn in_unpromoted_becomes_or_of_containment() {
    let c = compile(&one("remark", Op::In, json!(["a", "b"])));
    assert!(c.where_sql.contains("attrs @> $1::jsonb"));
    assert!(c.where_sql.contains("attrs @> $2::jsonb"));
    assert!(c.where_sql.contains(" OR "), "got: {}", c.where_sql);
}

#[test]
fn in_unpromoted_over_limit_rejected() {
    let vals: Vec<Value> = (0..33).map(|i| json!(i)).collect();
    let err = QueryBuilder::compile(td(), &one("remark", Op::In, Value::Array(vals))).unwrap_err();
    assert_eq!(err.code(), "MDL-QRY-IN-TOO-MANY");
}

#[test]
fn like_unpromoted_rejected() {
    let err = QueryBuilder::compile(td(), &one("remark", Op::Like, json!("%x%"))).unwrap_err();
    assert_eq!(err.code(), "MDL-QRY-PROMOTE-REQUIRED");
}

#[test]
fn range_promoted_uses_physical_col_with_numeric_cast() {
    let c = compile(&one("weight", Op::Ge, json!(1.5)));
    assert!(
        c.where_sql.contains("p_weight >= $1::numeric"),
        "got: {}",
        c.where_sql
    );
}

#[test]
fn isnull_unpromoted_uses_jsonb_has_key_negation() {
    let c = compile(&one("remark", Op::IsNull, json!(null)));
    assert!(
        c.where_sql.contains("NOT (attrs ? 'remark')"),
        "got: {}",
        c.where_sql
    );
}

#[test]
fn system_column_eq_direct() {
    let c = compile(&one("state", Op::Eq, json!("working")));
    assert!(
        c.where_sql.contains("state = $1::text"),
        "got: {}",
        c.where_sql
    );
}

#[test]
fn sort_promoted_ok_sort_unpromoted_rejected() {
    let q = ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![],
        sorts: vec![core_model::query::Sort {
            attr: "material".into(),
            desc: true,
        }],
        page: None,
    };
    let c = compile(&q);
    assert!(c.order_sql.contains("p_material DESC"));

    let bad = ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![],
        sorts: vec![core_model::query::Sort {
            attr: "remark".into(),
            desc: false,
        }],
        page: None,
    };
    let err = QueryBuilder::compile(td(), &bad).unwrap_err();
    assert_eq!(err.code(), "MDL-QRY-PROMOTE-REQUIRED");
}

#[test]
fn page_too_large_rejected_and_default_sort_present() {
    let err = QueryBuilder::compile(
        td(),
        &ObjectQuery {
            type_code: "drawing".into(),
            conds: vec![],
            sorts: vec![],
            page: Some(Page { page: 1, size: 501 }),
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), "MDL-QRY-PAGE-TOO-LARGE");

    let c = compile(&ObjectQuery::default());
    assert!(c.order_sql.contains("updated_at DESC"));
}

#[test]
fn unknown_attr_rejected() {
    let err = QueryBuilder::compile(td(), &one("nope", Op::Eq, json!(1))).unwrap_err();
    assert_eq!(err.code(), "MDL-ATTR-NOT-FOUND");
}

#[test]
fn pagination_math() {
    let c = compile(&ObjectQuery {
        type_code: "drawing".into(),
        conds: vec![],
        sorts: vec![],
        page: Some(Page { page: 3, size: 50 }),
    });
    assert_eq!(c.limit, 50);
    assert_eq!(c.offset, 100);
}
