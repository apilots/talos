// @vitest-environment jsdom
import { createRequire } from "node:module";
import { createCanvas } from "@napi-rs/canvas";
import { afterEach, expect, it, vi } from "vitest";

// Exercise the pinned renderer, including its real pixel-copy behavior.
const { default: Display } = createRequire(import.meta.url)("@novnc/novnc/lib/display") as {
  default: new (target: unknown) => {
    resize(width: number, height: number): void;
    fillRect(x: number, y: number, width: number, height: number, color: number[]): void;
    flip(): void;
    clipViewport: boolean;
    scale: number;
    viewportChangeSize(width: number, height: number): void;
    viewportChangePos(deltaX: number, deltaY: number): void;
  };
};

afterEach(() => vi.restoreAllMocks());

it("skips blank startup copies, renders the first framebuffer and preserves later resize pixels", () => {
  const canvas = () => Object.assign(createCanvas(300, 150), { style: {} });
  const target = canvas(), backbuffer = canvas();
  const context = backbuffer.getContext("2d");
  const readback = vi.spyOn(context, "getImageData");
  const visible = target.getContext("2d");
  const blit = vi.spyOn(visible, "drawImage");
  vi.spyOn(document, "createElement").mockReturnValue(backbuffer as unknown as HTMLElement);
  const display = new Display(target);

  display.resize(1440, 900);
  expect(readback).not.toHaveBeenCalled();
  expect(blit).not.toHaveBeenCalled();
  expect([target.width, target.height]).toEqual([1440, 900]);

  display.fillRect(4, 5, 1, 1, [18, 52, 86]);
  display.flip();
  expect(blit).toHaveBeenCalledOnce();
  expect([...visible.getImageData(4, 5, 1, 1).data]).toEqual([18, 52, 86, 255]);
  display.resize(1600, 1000);
  expect(readback).toHaveBeenCalledExactlyOnceWith(0, 0, 1440, 900);
  expect(blit).toHaveBeenCalledTimes(2);
  expect([...target.getContext("2d").getImageData(4, 5, 1, 1).data]).toEqual([18, 52, 86, 255]);
  display.resize(1600, 1000);
  expect(readback).toHaveBeenCalledTimes(1);

  display.clipViewport = true;
  display.viewportChangeSize(100, 80);
  display.viewportChangePos(4, 5);
  display.scale = 0.5;
  expect([target.width, target.height]).toEqual([100, 80]);
  expect(target.style).toMatchObject({ width: "50px", height: "40px" });
  expect([...visible.getImageData(0, 0, 1, 1).data]).toEqual([18, 52, 86, 255]);
});
