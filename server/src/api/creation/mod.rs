mod dto;

pub use dto::*;

use agent::{
    agent::{CallModelEvent, call_model},
    core::Message,
};
use axum::Json;
use futures::StreamExt;
use story_engine::utils::{build_chat_model, parse_json_response};

use crate::error::AppError;

use super::dto::ApiResponse;

type ApiResult<T> = Result<Json<ApiResponse<T>>, AppError>;

const CREATION_TRAIT_MIN: u8 = 1;
const CREATION_TRAIT_MAX: u8 = 10;
const CREATION_TRAIT_TOTAL: u16 = 30;

pub async fn generate_creation_draft(
    Json(request): Json<GenerateCreationDraftRequest>,
) -> ApiResult<GenerateCreationDraftData> {
    let target = request.target;
    let mut model = build_chat_model();
    model.set_output_json(true);

    let messages = vec![
        Message::system(
            r#"你是一名互动文字游戏创建页的 AI 表单生成器，负责生成可直接填入表单的中文内容。

总体目标：
1. 生成内容应偏女性向、女女关系潜力、都市奇幻/都市异能/豪门职场/娱乐圈/校园怪谈/近未来等方向，但每次都要有不同组合，不要固定套路。
2. 内容用于长期互动叙事的开局表单，不是人物小传或世界百科。字段要精炼、具体、可演绎，有欲望、秘密、代价、关系张力或异常规则。
3. 所有角色必须是成年人；不要生成露骨性内容、未成年人恋爱、现实名人、真实品牌或侵权 IP。
4. 可以参考用户当前表单来避开重复，并让新内容与当前另一组表单能够搭配；不要照抄当前表单。
5. 只输出一个合法 JSON 对象，不要代码块、标题、解释、注释或对象外文本。

当 target 是 "character" 时，输出结构必须是：
{
  "character": {
    "name": "2到4个汉字的中文名",
    "gender": "女",
    "age": 18到38之间的整数,
    "background": "一句命运烙印，12到32个中文字符，带明确钩子",
    "appearance": "45到110个中文字符的人物描述，包含外貌、气质、弱点或行动习惯",
    "traits": {
      "intellect": 1到10的整数,
      "physique": 1到10的整数,
      "endurance": 1到10的整数,
      "courage": 1到10的整数,
      "rationality": 1到10的整数,
      "altruism": 1到10的整数
    }
  }
}

character 约束：
- 六项 traits 总和必须恰好等于 30。
- 不要生成完美六边形；至少有一项不高于 3，至少有一项不低于 7。
- background 要像“命运烙印”，不是职业标签，例如可以包含契约、秘密、异能、旧爱、财阀、女巫、影后、白月光、记忆错位等元素。
- appearance 要能直接显示在表单里，保持一段话，不要分点。

当 target 是 "world" 时，输出结构必须是：
{
  "world": {
    "era": "4到24个中文字符的世界背景名",
    "description": "80到180个中文字符的世界描述，一段话，说明秩序、异常机制、主要压力和故事张力"
  }
}

world 约束：
- era 要适合放在下拉输入框里，不能太长。
- description 要能直接填入创建页文本框，具体但不冗长。
- 世界应天然适配女性主角与女女关系张力，但不要把所有冲突都写成恋爱。"#,
        ),
        Message::user(build_creation_draft_prompt(&request)),
    ];

    let mut stream = std::pin::pin!(call_model(&model, &messages, None));

    while let Some(event) = stream.next().await {
        match event {
            CallModelEvent::Completed { content, .. } => {
                let data = parse_generated_creation_draft(&content, target).map_err(|message| {
                    AppError::internal(format!("模型返回的创建表单格式不符合预期：{message}"))
                })?;
                return Ok(Json(ApiResponse::ok(data)));
            }
            CallModelEvent::Error(message) => {
                return Err(AppError::internal(format!("生成创建表单失败：{message}")));
            }
            CallModelEvent::TextChunk(_) | CallModelEvent::ReasoningChunk(_) => {}
        }
    }

    Err(AppError::internal("模型未返回完整创建表单。"))
}

fn build_creation_draft_prompt(request: &GenerateCreationDraftRequest) -> String {
    let target = match request.target {
        GenerateCreationDraftTarget::Character => "character",
        GenerateCreationDraftTarget::World => "world",
    };

    format!(
        r#"target: {target}

[当前人物表单]
- 姓名：{name}
- 性别：{gender}
- 年龄：{age}
- 命运烙印：{background}
- 人物描述：{appearance}
- 属性：智力 {intellect}，体力 {physique}，耐力 {endurance}，勇气 {courage}，理性 {rationality}，利他 {altruism}

[当前世界表单]
- 世界背景：{era}
- 世界描述：{description}

请根据 target 只生成对应的表单片段。"#,
        name = empty_placeholder(&request.character.name),
        gender = empty_placeholder(&request.character.gender),
        age = request.character.age,
        background = empty_placeholder(&request.character.background),
        appearance = empty_placeholder(&request.character.appearance),
        intellect = request.character.traits.intellect,
        physique = request.character.traits.physique,
        endurance = request.character.traits.endurance,
        courage = request.character.traits.courage,
        rationality = request.character.traits.rationality,
        altruism = request.character.traits.altruism,
        era = empty_placeholder(&request.world.era),
        description = empty_placeholder(&request.world.description),
    )
}

fn empty_placeholder(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "未填写".to_string()
    } else {
        value.to_string()
    }
}

fn parse_generated_creation_draft(
    content: &str,
    target: GenerateCreationDraftTarget,
) -> Result<GenerateCreationDraftData, String> {
    let parsed = parse_json_response::<GenerateCreationDraftData>(content)?;
    validate_generated_creation_draft(parsed, target)
}

fn validate_generated_creation_draft(
    data: GenerateCreationDraftData,
    target: GenerateCreationDraftTarget,
) -> Result<GenerateCreationDraftData, String> {
    match target {
        GenerateCreationDraftTarget::Character => {
            let character = data
                .character
                .ok_or_else(|| "`character` 不能为空。".to_string())?;
            Ok(GenerateCreationDraftData {
                character: Some(validate_creation_character(character)?),
                world: None,
            })
        }
        GenerateCreationDraftTarget::World => {
            let world = data.world.ok_or_else(|| "`world` 不能为空。".to_string())?;
            Ok(GenerateCreationDraftData {
                character: None,
                world: Some(validate_creation_world(world)?),
            })
        }
    }
}

fn validate_creation_character(character: CreationCharacter) -> Result<CreationCharacter, String> {
    let name = trim_required(character.name, "`character.name`")?;
    let gender = normalize_creation_gender(character.gender);
    let age = character.age.clamp(18, 80);
    let background = trim_required(character.background, "`character.background`")?;
    let appearance = trim_required(character.appearance, "`character.appearance`")?;
    let traits = normalize_creation_traits(character.traits);

    Ok(CreationCharacter {
        name,
        gender,
        age,
        appearance,
        traits,
        background,
    })
}

fn validate_creation_world(world: CreationWorld) -> Result<CreationWorld, String> {
    let era = trim_required(world.era, "`world.era`")?;
    let description = trim_required(world.description, "`world.description`")?;

    Ok(CreationWorld { era, description })
}

fn normalize_creation_gender(gender: String) -> String {
    match gender.trim() {
        "男" => "男".to_string(),
        _ => "女".to_string(),
    }
}

fn trim_required(value: String, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} 不能为空。"))
    } else {
        Ok(value.to_string())
    }
}

fn normalize_creation_traits(traits: CreationTraits) -> CreationTraits {
    let mut values = [
        traits
            .intellect
            .clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
        traits
            .physique
            .clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
        traits
            .endurance
            .clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
        traits.courage.clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
        traits
            .rationality
            .clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
        traits
            .altruism
            .clamp(CREATION_TRAIT_MIN, CREATION_TRAIT_MAX),
    ];

    let mut total = values.iter().map(|value| u16::from(*value)).sum::<u16>();
    while total < CREATION_TRAIT_TOTAL {
        for value in &mut values {
            if total >= CREATION_TRAIT_TOTAL {
                break;
            }
            if *value < CREATION_TRAIT_MAX {
                *value += 1;
                total += 1;
            }
        }
    }

    while total > CREATION_TRAIT_TOTAL {
        for value in values.iter_mut().rev() {
            if total <= CREATION_TRAIT_TOTAL {
                break;
            }
            if *value > CREATION_TRAIT_MIN {
                *value -= 1;
                total -= 1;
            }
        }
    }

    CreationTraits {
        intellect: values[0],
        physique: values[1],
        endurance: values[2],
        courage: values[3],
        rationality: values[4],
        altruism: values[5],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_generated_creation_character_trims_and_accepts_valid_traits() {
        let parsed = parse_generated_creation_draft(
            r#"{
                "character": {
                    "name": " 林昼雪 ",
                    "gender": "女",
                    "age": 24,
                    "appearance": " 黑色齐肩发，习惯把工牌藏进外套口袋。 ",
                    "background": " 与霸道女总裁签下隐婚契约的普通女孩 ",
                    "traits": {
                        "intellect": 8,
                        "physique": 3,
                        "endurance": 4,
                        "courage": 5,
                        "rationality": 6,
                        "altruism": 4
                    }
                }
            }"#,
            GenerateCreationDraftTarget::Character,
        )
        .expect("character draft should parse");

        let character = parsed.character.expect("character payload");
        assert_eq!(character.name, "林昼雪");
        assert_eq!(character.background, "与霸道女总裁签下隐婚契约的普通女孩");
        assert!(parsed.world.is_none());
    }

    #[test]
    fn parse_generated_creation_character_normalizes_trait_total() {
        let parsed = parse_generated_creation_draft(
            r#"{
                "character": {
                    "name": "林昼雪",
                    "gender": "未知",
                    "age": 12,
                    "appearance": "黑色齐肩发，习惯把工牌藏进外套口袋。",
                    "background": "与霸道女总裁签下隐婚契约的普通女孩",
                    "traits": {
                        "intellect": 11,
                        "physique": 0,
                        "endurance": 4,
                        "courage": 5,
                        "rationality": 6,
                        "altruism": 3
                    }
                }
            }"#,
            GenerateCreationDraftTarget::Character,
        )
        .expect("character draft should be normalized");

        let character = parsed.character.expect("character payload");
        let traits = character.traits;
        let total = u16::from(traits.intellect)
            + u16::from(traits.physique)
            + u16::from(traits.endurance)
            + u16::from(traits.courage)
            + u16::from(traits.rationality)
            + u16::from(traits.altruism);

        assert_eq!(character.gender, "女");
        assert_eq!(character.age, 18);
        assert_eq!(total, CREATION_TRAIT_TOTAL);
        assert!(
            [
                traits.intellect,
                traits.physique,
                traits.endurance,
                traits.courage,
                traits.rationality,
                traits.altruism
            ]
            .iter()
            .all(|value| (CREATION_TRAIT_MIN..=CREATION_TRAIT_MAX).contains(value))
        );
    }

    #[test]
    fn parse_generated_creation_world_trims_visible_fields() {
        let parsed = parse_generated_creation_draft(
            r#"{
                "world": {
                    "era": " 都市奇幻 ",
                    "description": " 雨夜高楼间藏着古老结界，女巫与财阀共同维持秘密秩序。 "
                }
            }"#,
            GenerateCreationDraftTarget::World,
        )
        .expect("world draft should parse");

        let world = parsed.world.expect("world payload");
        assert_eq!(world.era, "都市奇幻");
        assert_eq!(
            world.description,
            "雨夜高楼间藏着古老结界，女巫与财阀共同维持秘密秩序。"
        );
        assert!(parsed.character.is_none());
    }
}
