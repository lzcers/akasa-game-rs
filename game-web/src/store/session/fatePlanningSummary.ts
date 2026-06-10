import type { JsonValue } from './jsonValue';
import {
  isJsonObject,
  readJsonBoolean,
  readJsonNumber,
  readJsonString,
  readJsonStringArray,
} from './jsonValue';

interface FatePlanningSummary {
  round: number | null;
  sceneTitle: string | null;
  locationName: string | null;
  locationStatus: string | null;
  description: string | null;
  currentEvent: string | null;
  newInfo: string[];
  isEnding: boolean | null;
  endingType: string | null;
  protagonistCondition: string | null;
}

export function summarizeFatePlanning(value: JsonValue | null): FatePlanningSummary | null {
  if (!isJsonObject(value)) {
    return null;
  }

  return {
    round: readJsonNumber(value.round),
    sceneTitle: readJsonString(value.scene_title),
    locationName: readJsonString(value.location_name),
    locationStatus: readJsonString(value.location_status),
    description: readJsonString(value.description),
    currentEvent: readJsonString(value.current_event),
    newInfo: readJsonStringArray(value.new_info),
    isEnding: readJsonBoolean(value.is_ending),
    endingType: readJsonString(value.ending_type),
    protagonistCondition: readJsonString(value.protagonist_condition),
  };
}
