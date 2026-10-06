import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OnboardingView } from "../../src/views/OnboardingView";
import * as api from "../../src/lib/api";
import { transcript } from "./fixtures";

vi.mock("../../src/lib/api", () => ({
  checkMicrophone: vi.fn(),
  openHelpLink: vi.fn(),
  saveOpenAiApiKey: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(api.saveOpenAiApiKey).mockResolvedValue();
  vi.mocked(api.checkMicrophone).mockResolvedValue({ available: true });
  vi.mocked(api.openHelpLink).mockResolvedValue();
});

describe("guided setup", () => {
  it("keeps the user on the key step when credential storage fails", async () => {
    const user = userEvent.setup();
    const onKeyStatusChange = vi.fn();
    const onFinish = vi.fn();
    vi.mocked(api.saveOpenAiApiKey).mockRejectedValue(new Error("Sensitive internal failure"));
    render(<OnboardingView hasKey={false} snapshot={{ state: "idle" }} onKeyStatusChange={onKeyStatusChange} onFinish={onFinish} />);
    const input = screen.getByLabelText("OpenAI API key");
    await user.type(input, "sk-test-only-not-a-real-key");
    await user.click(screen.getByRole("button", { name: "Save API key" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Your setup has not advanced");
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    expect(input).toHaveValue("sk-test-only-not-a-real-key");
    expect(onKeyStatusChange).not.toHaveBeenCalled();
    expect(onFinish).not.toHaveBeenCalled();
    expect(screen.queryByText("Sensitive internal failure")).not.toBeInTheDocument();
  });

  it("clears the entered key only after a successful save and permits continuing", async () => {
    const user = userEvent.setup();
    function SetupHarness() {
      const [hasKey, setHasKey] = useState(false);
      return <OnboardingView hasKey={hasKey} snapshot={{ state: "idle" }} onKeyStatusChange={setHasKey} onFinish={vi.fn()} />;
    }
    render(<SetupHarness />);
    await user.type(screen.getByLabelText("OpenAI API key"), "sk-test-only-not-a-real-key");
    await user.click(screen.getByRole("button", { name: "Save API key" }));

    expect(await screen.findByText(/API key saved in Windows Credential Manager/)).toBeInTheDocument();
    expect(screen.queryByLabelText("OpenAI API key")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Continue" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("heading", { name: /Set up Vellora.*Microphone/ })).toBeInTheDocument();
    expect(api.checkMicrophone).not.toHaveBeenCalled();
  });

  it("checks microphone availability only after an explicit click and reports failure", async () => {
    const user = userEvent.setup();
    vi.mocked(api.checkMicrophone).mockResolvedValue({ available: false });
    render(<OnboardingView hasKey snapshot={{ state: "idle" }} onKeyStatusChange={vi.fn()} onFinish={vi.fn()} />);
    expect(screen.getByRole("heading", { name: /Set up Vellora.*OpenAI key/ })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(api.checkMicrophone).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Check microphone" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("default microphone could not be confirmed");
    expect(api.checkMicrophone).toHaveBeenCalledOnce();
    expect(api.saveOpenAiApiKey).not.toHaveBeenCalled();
  });

  it("allows skipping immediately without saving a key or checking the microphone", async () => {
    const user = userEvent.setup();
    const onFinish = vi.fn();
    render(<OnboardingView hasKey={false} snapshot={{ state: "idle" }} onKeyStatusChange={vi.fn()} onFinish={onFinish} />);
    await user.click(screen.getByRole("button", { name: "Skip for now" }));
    expect(onFinish).toHaveBeenCalledWith("skipped");
    expect(api.saveOpenAiApiKey).not.toHaveBeenCalled();
    expect(api.checkMicrophone).not.toHaveBeenCalled();
  });

  it("does not count an old transcript as practice and can finish without a paid recording", async () => {
    const user = userEvent.setup();
    const onFinish = vi.fn();
    const props = { hasKey: true, snapshot: { state: "done" as const, lastTranscript: transcript({ id: "old" }) }, onKeyStatusChange: vi.fn(), onFinish };
    const view = render(<OnboardingView {...props} />);
    for (let index = 0; index < 3; index++) await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.queryByText(/A new transcript was created/)).not.toBeInTheDocument();

    view.rerender(<OnboardingView {...props} snapshot={{ state: "done", lastTranscript: transcript({ id: "new" }) }} />);
    await waitFor(() => expect(screen.getByText(/A new transcript was created/)).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Finish setup" }));
    expect(onFinish).toHaveBeenCalledWith("complete");
    expect(api.saveOpenAiApiKey).not.toHaveBeenCalled();
    expect(api.checkMicrophone).not.toHaveBeenCalled();
  });
});
