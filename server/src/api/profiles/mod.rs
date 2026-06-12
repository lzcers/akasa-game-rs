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

pub async fn generate_profiles(
    Json(request): Json<GenerateProfilesRequest>,
) -> ApiResult<GenerateProfilesData> {
    let mut model = build_chat_model();
    model.set_output_json(true);

    let messages = vec![
        Message::system(
            r#"你是一名“互动叙事世界圣经生成器”，负责把用户给出的角色创建表单扩写为可长期使用的世界设定与玩家角色设定。

你的目标不是重写用户想法，而是在严格遵守现有输入的前提下，把它整理、补完并提升为更适合故事展开、冲突升级和多轮演绎的设定文本。

请严格遵守以下规则：
1. 用户已经填写的内容全部视为硬约束，不得擅自修改、替换或否定，包括但不限于：姓名、性别、年龄、外貌/标记、三项特质、人生烙印、时代。
2. 你可以补充背景细节、社会氛围、势力结构、人物心理、行为倾向、潜在代价、关系压力，但这些补充必须服务于用户已给出的核心设定。
3. 三项特质数值不是要被机械复述，而是要转化为玩家角色的行为倾向、判断方式、优势与弱点。你可以体现“更容易如何行动、在压力下会如何失衡”，但不要把文本写成属性说明书。
4. 文本风格应偏文学叙事，要求有画面感、情绪和张力，但不能空泛。每一句都应尽量服务于后续剧情推进、角色抉择或世界冲突。
5. 设定必须有利于长期演绎。要让后续故事中自然存在：阻力、代价、欲望、误判空间、势力压迫、道德撕扯或命运反噬。
6. 不要生成万能、无敌、没有代价的玩家角色设定；不要生成过于封闭、没有可持续冲突的世界设定；不要写成百科、总结提纲、游戏数值说明或策划备注。

请按以下 JSON 结构生成，字段名必须完全一致：
{
  "world": "世界设定正文",
  "character": "玩家角色设定正文",
  "keyStoryBeats": "关键节点骨架，多行字符串，每行一个节点"
}

字段要求：
1. `world`：
- 用一段完整、连贯的中文正文书写。
- 必须包含并自然融合以下内容：
  - 时代气质与整体氛围。
  - 支配这个世界的关键规则、禁忌、秩序或异常机制。
  - 与核心矛盾直接相关的主要势力、压迫来源或冲突结构。
  - 让故事得以持续展开的现实张力，例如代价、风险、失衡、猜疑、资源争夺、身份压力等。
- 重点不是堆设定，而是建立一个能不断逼迫人物做选择的世界。

2. `character`：
- 用一段完整、连贯的中文正文书写。
- 必须包含并自然融合以下内容：
  - 用户给定身份、外貌、人生烙印在这个世界中的意义。
  - 玩家角色最强烈的欲望、执念或驱动力。
  - 玩家角色最明显的弱点、裂缝、恐惧、盲点或代价来源。
  - 三项特质如何转化为玩家角色的行动风格与决策倾向。
  - 玩家角色为何会被卷入世界主冲突，以及其处境为何适合长期演绎。
- 重点不是称赞玩家角色，而是让玩家角色既有魅力，也有会在剧情中不断出问题的地方。

3. `keyStoryBeats`：
- 写成 4 到 6 行的多行字符串，每行一个关键节点，不要编号。
- 每一条都应是故事未来必须抵达、对角色与世界都具有决定性影响的关键场景、真相揭示、关系断裂、代价兑现或终局瞬间。
- 这些节点不是详细剧情梗概，而是“结局引力场”的骨架，要足够具体，能为后续 FateWeaver 提供持续牵引。
- 节点之间应体现递进关系：前期埋入牵引，中期扩大代价，后期逼近不可回避的抉择与收束。
- 至少包含：
  - 一个会重新定义玩家角色自我认知或身份位置的节点。
  - 一个让世界主冲突彻底升级、无法继续回避的节点。
  - 一个具有明确终局气息的收束节点。

输出要求：
1. 只输出一个合法 JSON 对象。
2. 不要输出代码块，不要输出额外标题、解释、分析、注释或对象外文本。
3. 三个字段都必须是非空字符串。
4. `keyStoryBeats` 必须是多行字符串，且每行都是一个独立节点。
5. 两段正文都要具体、克制、可演绎，避免空话、套话和泛泛而谈。"#,
        ),
        Message::user(build_generate_profiles_prompt(&request)),
    ];

    let mut stream = std::pin::pin!(call_model(&model, &messages, None));

    while let Some(event) = stream.next().await {
        match event {
            CallModelEvent::Completed { content, .. } => {
                let data = parse_generated_profiles(&content).map_err(|message| {
                    AppError::internal(format!("模型返回格式不符合预期：{message}"))
                })?;
                return Ok(Json(ApiResponse::ok(data)));
            }
            CallModelEvent::Error(message) => {
                return Err(AppError::internal(format!("生成设定失败：{message}")));
            }
            CallModelEvent::TextChunk(_) | CallModelEvent::ReasoningChunk(_) => {}
        }
    }

    Err(AppError::internal("模型未返回完整结果。"))
}

fn build_generate_profiles_prompt(request: &GenerateProfilesRequest) -> String {
    format!(
        r#"请基于以下两组设定，像从“阿卡夏记录”中与玩家输入共鸣一样，生成“世界记录”和“角色记录”。

这些表单内容都是已确定事实，禁止改写、替换或否定，只能围绕它们做扩写、补完和强化。生成结果应像记录被唤醒、世界与角色逐步显影，而不是普通设定简介。

[角色记录种子]
- 姓名：{name}
- 性别：{gender}
- 年龄：{age}
- 角色烙印：{background}
- 角色描述：{appearance}
- 属性分配：
  - 智力：{intellect}
  - 体力：{physique}
  - 耐力：{endurance}
  - 勇气：{courage}
  - 理性：{rationality}
  - 利他：{altruism}

[世界记录种子]
- 时代：{era}
- 世界记录：{description}

[生成目标]
- 这是长期 AI 互动小说的记录底稿，不是一次性简介。
- 世界记录必须严格建立在“世界记录种子”事实上。
- 角色记录必须严格建立在“角色记录种子”事实上，并自然解释角色为何会被卷入这个故事。
- 世界记录重点写清世界如何运转、现实压力从何而来，以及什么样的秩序正在支配众人。
- 角色记录重点写清欲望、弱点、行动倾向，以及六项属性如何转化为行为习惯与判断方式。
- 语气可以带有“记录、共鸣、显影、回响”的阿卡夏感，但不要堆砌术语。"#,
        name = request.character.name.trim(),
        gender = request.character.gender.trim(),
        age = request.character.age,
        background = empty_placeholder(&request.character.background),
        appearance = empty_placeholder(&request.character.appearance),
        intellect = request.character.traits.intellect,
        physique = request.character.traits.physique,
        endurance = request.character.traits.endurance,
        courage = request.character.traits.courage,
        rationality = request.character.traits.rationality,
        altruism = request.character.traits.altruism,
        era = request.world.era.trim(),
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

fn parse_generated_profiles(content: &str) -> Result<GenerateProfilesData, String> {
    let parsed = parse_json_response::<GenerateProfilesData>(content)?;
    validate_generated_profiles(parsed)
}

fn validate_generated_profiles(data: GenerateProfilesData) -> Result<GenerateProfilesData, String> {
    let world = data.world.trim();
    if world.is_empty() {
        return Err("`world` 不能为空。".to_string());
    }

    let character = data.character.trim();
    if character.is_empty() {
        return Err("`character` 不能为空。".to_string());
    }

    let beats = data.key_story_beats.trim();
    if beats.is_empty() {
        return Err("`keyStoryBeats` 不能为空。".to_string());
    }

    Ok(GenerateProfilesData {
        world: world.to_string(),
        character: character.to_string(),
        key_story_beats: beats.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::creation::{CreationCharacter, CreationTraits, CreationWorld};

    fn profiles_request() -> GenerateProfilesRequest {
        GenerateProfilesRequest {
            character: CreationCharacter {
                name: " 叶知秋 ".to_string(),
                gender: " 女 ".to_string(),
                age: 28,
                appearance: "总把银色发针别在袖口".to_string(),
                traits: CreationTraits {
                    intellect: 8,
                    physique: 3,
                    endurance: 4,
                    courage: 6,
                    rationality: 7,
                    altruism: 2,
                },
                background: "被旧神契约标记的人".to_string(),
            },
            world: CreationWorld {
                era: " 雨夜财阀异能 ".to_string(),
                description: "高楼之间的契约会吞掉违约者的名字。".to_string(),
            },
        }
    }

    #[test]
    fn build_generate_profiles_prompt_uses_structured_form_fields() {
        let prompt = build_generate_profiles_prompt(&profiles_request());

        assert!(prompt.contains("- 姓名：叶知秋"));
        assert!(prompt.contains("- 性别：女"));
        assert!(prompt.contains("- 年龄：28"));
        assert!(prompt.contains("- 角色烙印：被旧神契约标记的人"));
        assert!(prompt.contains("- 时代：雨夜财阀异能"));
        assert!(prompt.contains("- 智力：8"));
        assert!(!prompt.contains("prompt"));
    }

    #[test]
    fn build_generate_profiles_prompt_marks_blank_optional_text() {
        let mut request = profiles_request();
        request.character.background = "  ".to_string();
        request.character.appearance.clear();
        request.world.description = "\n".to_string();

        let prompt = build_generate_profiles_prompt(&request);

        assert!(prompt.contains("- 角色烙印：未填写"));
        assert!(prompt.contains("- 角色描述：未填写"));
        assert!(prompt.contains("- 世界记录：未填写"));
    }

    #[test]
    fn parse_generated_profiles_trims_visible_fields() {
        let parsed = parse_generated_profiles(
            r#"{
                "world": " 雨幕之下，契约成为城市真正的法律。 ",
                "character": " 叶知秋习惯以理性遮掩恐惧。 ",
                "keyStoryBeats": "她第一次听见被抹去者的名字\n财阀公开撕毁旧契约"
            }"#,
        )
        .expect("profiles should parse");

        assert_eq!(parsed.world, "雨幕之下，契约成为城市真正的法律。");
        assert_eq!(parsed.character, "叶知秋习惯以理性遮掩恐惧。");
        assert_eq!(
            parsed.key_story_beats,
            "她第一次听见被抹去者的名字\n财阀公开撕毁旧契约"
        );
    }
}
