import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HotWord } from "@/types";

const api = vi.hoisted(() => ({ preview: vi.fn(), add: vi.fn(), remove: vi.fn(), saved: vi.fn(), close: vi.fn() }));
vi.mock("@/api/vocabulary", () => ({ previewHotWords: api.preview, addHotWords: api.add, removeHotWords: api.remove }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, args?: Record<string, unknown>) => `${key}${args ? ` ${JSON.stringify(args)}` : ""}` }) }));
import VocabularyManager from "../VocabularyManager";

const words: HotWord[] = Array.from({ length: 650 }, (_, index) => ({
  text: `设备 ${String(index).padStart(3, "0")}`, weight: 3, source: "user", use_count: 0, last_used: 0,
}));
const button = (name: string) => screen.getByRole("button", { name: new RegExp(name) });
beforeEach(() => {
  vi.clearAllMocks();
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  api.saved.mockResolvedValue(undefined);
  api.preview.mockResolvedValue({ words: ["SPC 工作站", "狗窝检具"], duplicates: 1, invalid: ["x".repeat(81)] });
  api.add.mockResolvedValue({ added: 2, duplicates: 1, invalid: ["x".repeat(81)] });
  api.remove.mockResolvedValue(1);
});
const open = () => render(<VocabularyManager words={words} onSaved={api.saved} onClose={api.close} />);

describe("vocabulary manager", () => {
  it("paginates hundreds of terms and searches the entire vocabulary", () => {
    open();
    expect(screen.getAllByRole("checkbox")).toHaveLength(51);
    expect(screen.getByText("设备 000")).toBeInTheDocument();
    expect(screen.queryByText("设备 050")).not.toBeInTheDocument();
    fireEvent.click(button("vocabulary.next"));
    expect(screen.getByText("设备 050")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "设备 649" } });
    expect(screen.getByText("设备 649")).toBeInTheDocument();
    expect(screen.getAllByRole("checkbox")).toHaveLength(2);
  });

  it("previews pasted lines before submitting a single batch and preserves spaces", async () => {
    open();
    fireEvent.click(button("vocabulary.batchAdd"));
    const text = "SPC 工作站\n狗窝检具\nSPC 工作站\n" + "x".repeat(81);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: text } });
    expect(button("vocabulary.confirmAdd")).toBeDisabled();
    fireEvent.click(button("vocabulary.preview"));
    await waitFor(() => expect(button("vocabulary.confirmAdd")).not.toBeDisabled());
    expect(api.preview).toHaveBeenCalledWith(text);
    expect(screen.getByText("SPC 工作站")).toBeInTheDocument();
    fireEvent.click(button("vocabulary.confirmAdd"));
    await waitFor(() => expect(api.add).toHaveBeenCalledWith(text));
    await waitFor(() => expect(api.saved).toHaveBeenCalledOnce());
    expect(api.add).toHaveBeenCalledOnce();
  });

  it("keeps drafts after errors and invalidates preview when text changes", async () => {
    open();
    fireEvent.click(button("vocabulary.batchAdd"));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "SPC 工作站" } });
    fireEvent.click(button("vocabulary.preview"));
    await waitFor(() => expect(button("vocabulary.confirmAdd")).not.toBeDisabled());
    api.add.mockRejectedValueOnce(new Error("写入失败"));
    fireEvent.click(button("vocabulary.confirmAdd"));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("写入失败"));
    expect(screen.getByRole("textbox")).toHaveValue("SPC 工作站");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "PLC" } });
    expect(button("vocabulary.confirmAdd")).toBeDisabled();
  });

  it("deletes only explicitly confirmed terms and clears selection on page changes", async () => {
    open();
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(button("vocabulary.deleteSelected"));
    expect(api.remove).not.toHaveBeenCalled();
    fireEvent.click(button("vocabulary.confirmDelete"));
    await waitFor(() => expect(api.remove).toHaveBeenCalledWith(["设备 000"]));
    await waitFor(() => expect(api.saved).toHaveBeenCalledOnce());
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(button("vocabulary.next"));
    expect(button("vocabulary.deleteSelected")).toBeDisabled();
  });
});
