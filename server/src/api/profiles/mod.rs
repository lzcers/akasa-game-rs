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
    if request.prompt.trim().is_empty() {
        return Err(AppError::bad_request("`prompt` 不能为空。"));
    }

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
        Message::user(request.prompt),
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
