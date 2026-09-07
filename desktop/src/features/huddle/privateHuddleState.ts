import type {
  HuddleContextValue,
  HuddleLevelsValue,
} from "./HuddleContext.types";

const idle = () => {};
const unavailable = async () => {
  throw new Error("Huddles are unavailable in the private Ortak desktop.");
};

/** Inert context preserves Office consumers without mounting any voice hooks. */
export const privateHuddleState: HuddleContextValue = {
  localAudioTrack: null,
  isStarting: false,
  huddleError: null,
  clearHuddleError: idle,
  micConnected: false,
  isMuted: true,
  toggleMute: idle,
  interruptAgentSpeech: async () => {},
  pttActive: false,
  voiceInputMode: "voice_activity",
  setVoiceInputMode: unavailable,
  audioDevices: [],
  selectedDeviceId: "",
  setSelectedDeviceId: idle,
  micGain: 1,
  setMicGain: idle,
  outputDevices: [],
  selectedOutputDevice: "",
  setSelectedOutputDevice: idle,
  activeEphemeralChannelId: null,
  showHuddleInMainApp: idle,
  viewHuddleChannel: idle,
  startHuddle: unavailable,
  joinHuddle: unavailable,
  leaveHuddle: async () => true,
};

export const privateHuddleLevels: HuddleLevelsValue = {
  micLevel: 0,
  activeSpeakers: [],
  speakerLevels: {},
};
