import * as DialogPrimitive from "@radix-ui/react-dialog";
import * as AlertPrimitive from "@radix-ui/react-alert-dialog";
import { forwardRef, type ComponentPropsWithoutRef, type ElementRef } from "react";
import { cn } from "@/shared/lib/cn";

const overlayClass = "fixed inset-0 z-[1000] flex items-start justify-center overflow-y-auto bg-black/30 px-4 py-6 backdrop-blur-[2px] sm:py-10";
const contentClass = "relative my-auto w-full max-w-[520px] shrink-0 rounded-[var(--radius-panel)] border border-border bg-card p-6 text-ink shadow-panel outline-none";
export const Dialog = DialogPrimitive.Root;
export const DialogTitle = DialogPrimitive.Title;
export const DialogDescription = DialogPrimitive.Description;
export const DialogClose = DialogPrimitive.Close;
export const AlertDialog = AlertPrimitive.Root;
export const AlertDialogTitle = AlertPrimitive.Title;
export const AlertDialogDescription = AlertPrimitive.Description;
export const AlertDialogCancel = AlertPrimitive.Cancel;
export const AlertDialogAction = AlertPrimitive.Action;

export const DialogContent = forwardRef<ElementRef<typeof DialogPrimitive.Content>, ComponentPropsWithoutRef<typeof DialogPrimitive.Content>>(function DialogContent({ className, ...props }, ref) {
  return <DialogPrimitive.Portal><DialogPrimitive.Overlay className={overlayClass}><DialogPrimitive.Content {...props} className={cn(contentClass, className)} ref={ref} /></DialogPrimitive.Overlay></DialogPrimitive.Portal>;
});

export const AlertDialogContent = forwardRef<ElementRef<typeof AlertPrimitive.Content>, ComponentPropsWithoutRef<typeof AlertPrimitive.Content>>(function AlertDialogContent({ className, ...props }, ref) {
  return <AlertPrimitive.Portal><AlertPrimitive.Overlay className={overlayClass}><AlertPrimitive.Content {...props} className={cn(contentClass, className)} ref={ref} /></AlertPrimitive.Overlay></AlertPrimitive.Portal>;
});
