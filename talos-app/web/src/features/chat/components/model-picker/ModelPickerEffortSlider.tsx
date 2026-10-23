import * as Slider from "@radix-ui/react-slider";
import { Cat } from "lucide-react";
import type { CSSProperties } from "react";

import type { ReasoningLevel } from "@/shared/types/ui";
import styles from "./ModelPicker.module.css";

const DURATIONS: Record<ReasoningLevel, string> = { none: "0s", light: "4.8s", medium: "3.2s", high: "2s", extra_high: "1.2s" };

interface ModelPickerEffortSliderProps {
  disabled: boolean;
  label: string;
  levels: ReasoningLevel[];
  onChange?: (level: ReasoningLevel) => void;
  onPreviewChange: (level: ReasoningLevel) => void;
  value: ReasoningLevel;
  valueLabel: (level: ReasoningLevel) => string;
}

export function ModelPickerEffortSlider({ disabled, label, levels, onChange, onPreviewChange, value, valueLabel }: ModelPickerEffortSliderProps) {
  const index = Math.max(0, levels.indexOf(value));
  return (
    <Slider.Root className={styles.effortSlider} data-effort={value} disabled={disabled} max={Math.max(1, levels.length - 1)} min={0}
      onValueChange={([next]) => onPreviewChange(levels[next] ?? "none")}
      onValueCommit={([next]) => onChange?.(levels[next] ?? "none")} step={1}
      style={{ "--cat-duration": DURATIONS[value] } as CSSProperties} value={[index]}>
      <Slider.Track className={styles.effortTrack}>
        <Slider.Range className={styles.effortFill}>
          <span aria-hidden className={styles.catLane}><span className={styles.rainbowCat}><span className={styles.rainbowTail} /><Cat className={styles.catIcon} size={15} strokeWidth={1.8} /></span></span>
        </Slider.Range>
        <span aria-hidden className={styles.effortDots}>{levels.map((level, stop) => <span className={stop < index ? styles.covered : undefined} key={level} />)}</span>
      </Slider.Track>
      <Slider.Thumb aria-disabled={disabled} aria-label={label} aria-valuetext={valueLabel(value)} className={styles.effortThumb} title={valueLabel(value)} />
    </Slider.Root>
  );
}
