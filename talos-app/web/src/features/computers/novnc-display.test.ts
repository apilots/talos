// @vitest-environment jsdom
import { createRequire } from "node:module";
import { createCanvas } from "@napi-rs/canvas";
import { afterEach, expect, it, vi } from "vitest";

// Exercise the pinned renderer, including its real pixel-copy behavior.
const { default: Display } = createRequire(import.meta.url)("@novnc/novnc/lib/display") as {
  default: new (target: unknown) => { resize(width: number, height: number): void };
};

afterEach(() => vi.restoreAllMocks());

it("does not read back an uninitialized framebuffer, but preserves pixels on subsequent resize", () => {
  const canvas = () => Object.assign(createCanvas(300, 150), { style: {} });
  const target = canvas(), backbuffer = canvas();
  const context = backbuffer.getContext("2d");
  const readback = vi.spyOn(context, "getImageData");
  vi.spyOn(document, "createElement").mockReturnValue(backbuffer as unknown as HTMLElement);
  const display = new Display(target);

  display.resize(1440, 900);
  expect(readback).not.toHaveBeenCalled();
  expect([target.width, target.height]).toEqual([1440, 900]);

  context.fillStyle = "#123456";
  context.fillRect(4, 5, 1, 1);
  display.resize(1600, 1000);
  expect(readback).toHaveBeenCalledExactlyOnceWith(0, 0, 1440, 900);
  expect([...target.getContext("2d").getImageData(4, 5, 1, 1).data]).toEqual([18, 52, 86, 255]);
  display.resize(1600, 1000);
  expect(readback).toHaveBeenCalledTimes(1);
});
