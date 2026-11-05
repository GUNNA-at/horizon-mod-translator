use super::*;

#[test]
fn identifier_adds_default_namespace() {
    assert_eq!(identifier("stone").unwrap(), "minecraft:stone");
    assert_eq!(identifier("a:b").unwrap(), "a:b");
}

#[test]
fn positive_modulo_wraps_negatives() {
    assert!((positive_modulo(-0.25, 1.0) - 0.75).abs() < 1e-6);
    assert!((positive_modulo(1.5, 1.0) - 0.5).abs() < 1e-6);
}

#[test]
fn display_context_names_round_trip() {
    for name in ["gui", "ground", "fixed", "head"] {
        assert_eq!(DisplayContext::by_name(name).unwrap().name(), name);
    }
}

/// A translated mod's 1.21.1 `overrides` (`horizon:overrides`), as `ItemOverrides.resolve`
/// picks: the last override whose predicates all hold, a property no function answers is -inf
/// (never at a threshold, 0 included), and `damage` is vanilla's on a stack that can break.
#[test]
fn translated_overrides_pick_as_item_overrides_resolve() {
    /// The mod's own `pulling` and `pull`, when it registered them.
    struct Bow(Option<(f32, f32)>);
    impl ItemEnv for Bow {
        fn item_property(
            &self,
            _stack: &StackView,
            _components: &Components,
            id: &str,
            _ctx: &Context<'_>,
        ) -> Option<f32> {
            let (pulling, pull) = self.0?;
            match id {
                "minecraft:pulling" => Some(pulling),
                "minecraft:pull" => Some(pull),
                _ => None,
            }
        }
    }
    let leaf = |m: &str| serde_json::json!({"type": "minecraft:model", "model": m});
    let definition = serde_json::json!({"model": {
        "type": "horizon:overrides",
        "base": leaf("x:item/bow"),
        "overrides": [
            {"predicate": {"pulling": 1}, "model": leaf("x:item/bow_pulling_0")},
            {"predicate": {"pulling": 1, "pull": 0.65}, "model": leaf("x:item/bow_pulling_1")},
            {"predicate": {"pulling": 1, "pull": 0.9}, "model": leaf("x:item/bow_pulling_2")},
            {"predicate": {"x:unregistered": 0}, "model": leaf("x:item/never")},
            {"predicate": {"damage": 0.5}, "model": leaf("x:item/worn")},
        ],
    }});
    let mut models = ItemModels::default();
    let item = ClientItem::decode(&definition).unwrap();
    models.definitions.insert("x:bow".into(), item);
    let prototype = Components::from([(
        "minecraft:item_model".to_owned(),
        Value::String("x:bow".into()),
    )]);
    models.prototypes.insert("x:bow".into(), prototype);
    let ctx = Context {
        display: DisplayContext::Gui,
        owner: None,
        level: None,
        seed: 0,
    };
    let drawn = |env: &Bow, stack: &StackView| -> String {
        match &models.resolve(stack, &ctx, env).layers[0].content {
            LayerContent::Model { model } => model.clone(),
            other => panic!("{other:?}"),
        }
    };
    let bow = StackView::new("x:bow", 1);
    assert_eq!(drawn(&Bow(None), &bow), "x:item/bow");
    assert_eq!(drawn(&Bow(Some((0.0, 0.0))), &bow), "x:item/bow");
    assert_eq!(drawn(&Bow(Some((1.0, 0.3))), &bow), "x:item/bow_pulling_0");
    assert_eq!(drawn(&Bow(Some((1.0, 0.7))), &bow), "x:item/bow_pulling_1");
    assert_eq!(drawn(&Bow(Some((1.0, 0.95))), &bow), "x:item/bow_pulling_2");
    // 60 damage of 100 is past 0.5: the later override wins over the pull.
    let mut worn = StackView::new("x:bow", 1);
    worn.patch
        .insert("minecraft:max_damage".into(), Some(serde_json::json!(100)));
    worn.patch
        .insert("minecraft:damage".into(), Some(serde_json::json!(60)));
    assert_eq!(drawn(&Bow(Some((1.0, 0.95))), &worn), "x:item/worn");
}
