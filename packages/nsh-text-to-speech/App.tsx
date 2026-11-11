import { mount, signal, Text, View } from "nsh";
import { Speech, SpeechError } from "./src";

/** The device's own voice, through the Rust crate. */
const speech = new Speech();

const status = signal("idle");

function codeOf(error: unknown): string {
  return error instanceof SpeechError ? error.code : "unknown error";
}

function App() {
  const speak = () => {
    speech.speak("Hello from Native Shell").then(
      (id) => status.set(`speaking ${id}`),
      (error: unknown) => status.set(codeOf(error)),
    );
  };
  return (
    <View class="flex-1 gap-3 p-4">
      <Text testID="title" class="text-foreground">nsh-text-to-speech</Text>
      <Text testID="status" class="text-foreground">{status()}</Text>
      <Text testID="speak" role="button" onPress={speak} class="rounded-(--radius) bg-muted px-3 py-2">
        Speak
      </Text>
    </View>
  );
}

mount(App);
