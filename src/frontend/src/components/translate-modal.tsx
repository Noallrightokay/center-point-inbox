"use client";

import { useState } from "react";
import { Languages, CheckCircle, XCircle, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import {
  useRequestTranslation,
  useTranslationStatus,
} from "@/hooks/use-translation";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TARGET_LANGUAGES = [
  { value: "en", label: "English" },
  { value: "es", label: "Spanish" },
  { value: "fr", label: "French" },
  { value: "de", label: "German" },
  { value: "it", label: "Italian" },
  { value: "pt", label: "Portuguese" },
  { value: "zh", label: "Chinese (Simplified)" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
  { value: "ar", label: "Arabic" },
  { value: "hi", label: "Hindi" },
  { value: "ru", label: "Russian" },
  { value: "nl", label: "Dutch" },
  { value: "sv", label: "Swedish" },
  { value: "pl", label: "Polish" },
  { value: "tr", label: "Turkish" },
  { value: "vi", label: "Vietnamese" },
  { value: "th", label: "Thai" },
  { value: "he", label: "Hebrew" },
  { value: "uk", label: "Ukrainian" },
] as const;

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface TranslateModalProps {
  fileId: string;
  fileName: string;
  sourceLanguage?: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function TranslateModal({
  fileId,
  fileName,
  sourceLanguage,
  open,
  onOpenChange,
}: TranslateModalProps) {
  const [targetLanguage, setTargetLanguage] = useState("");
  const [phiRedaction, setPhiRedaction] = useState(false);
  const [jobId, setJobId] = useState<string | null>(null);

  const requestTranslation = useRequestTranslation();
  const { data: jobStatus } = useTranslationStatus(jobId);

  const isSubmitting = requestTranslation.isPending;
  const isProcessing =
    jobId !== null &&
    jobStatus?.status !== "completed" &&
    jobStatus?.status !== "failed";
  const isComplete = jobStatus?.status === "completed";
  const isFailed = jobStatus?.status === "failed";

  function handleSubmit() {
    if (!targetLanguage) return;

    requestTranslation.mutate(
      {
        fileId,
        targetLanguage,
        sourceLanguage: sourceLanguage ?? undefined,
        fieldsToTranslate: phiRedaction
          ? ["title", "body"]
          : undefined,
      },
      {
        onSuccess: (data) => {
          setJobId(data.id);
        },
      },
    );
  }

  function handleClose() {
    // Reset state when closing
    setTargetLanguage("");
    setPhiRedaction(false);
    setJobId(null);
    requestTranslation.reset();
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Languages className="h-5 w-5" />
            Translate File
          </DialogTitle>
          <DialogDescription>
            Translate <span className="font-medium">{fileName}</span> into
            another language.
            {sourceLanguage && (
              <span className="block mt-1">
                Detected language:{" "}
                <Badge variant="secondary" className="text-xs">
                  {sourceLanguage.toUpperCase()}
                </Badge>
              </span>
            )}
          </DialogDescription>
        </DialogHeader>

        {/* Idle / form state */}
        {!jobId && !isSubmitting && (
          <div className="space-y-4 py-2">
            {/* Target language */}
            <div className="space-y-2">
              <label
                htmlFor="target-language"
                className="text-sm font-medium leading-none"
              >
                Target Language
              </label>
              <Select
                value={targetLanguage}
                onValueChange={setTargetLanguage}
              >
                <SelectTrigger id="target-language">
                  <SelectValue placeholder="Select a language" />
                </SelectTrigger>
                <SelectContent>
                  {TARGET_LANGUAGES.map((lang) => (
                    <SelectItem
                      key={lang.value}
                      value={lang.value}
                      disabled={lang.value === sourceLanguage}
                    >
                      {lang.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* PHI redaction toggle */}
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div className="space-y-0.5">
                <label
                  htmlFor="phi-redaction"
                  className="text-sm font-medium leading-none cursor-pointer"
                >
                  PHI Redaction
                </label>
                <p className="text-xs text-muted-foreground">
                  Automatically redact protected health information before
                  translation.
                </p>
              </div>
              <button
                id="phi-redaction"
                role="switch"
                aria-checked={phiRedaction}
                onClick={() => setPhiRedaction(!phiRedaction)}
                className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full border-2 border-transparent transition-colors ${
                  phiRedaction ? "bg-primary" : "bg-muted"
                }`}
              >
                <span
                  className={`pointer-events-none block h-4 w-4 rounded-full bg-background shadow-lg transition-transform ${
                    phiRedaction ? "translate-x-4" : "translate-x-0"
                  }`}
                />
              </button>
            </div>
          </div>
        )}

        {/* Submitting state */}
        {isSubmitting && (
          <div className="flex flex-col items-center justify-center py-8 gap-3">
            <Loader2 className="h-8 w-8 animate-spin text-primary" />
            <p className="text-sm text-muted-foreground">
              Submitting translation request...
            </p>
          </div>
        )}

        {/* Processing state */}
        {isProcessing && (
          <div className="flex flex-col items-center justify-center py-8 gap-3">
            <Loader2 className="h-8 w-8 animate-spin text-primary" />
            <p className="text-sm text-muted-foreground">
              Translation in progress...
            </p>
            <Badge variant="secondary">
              {jobStatus?.status === "pending" ? "Queued" : "Processing"}
            </Badge>
          </div>
        )}

        {/* Success state */}
        {isComplete && (
          <div className="flex flex-col items-center justify-center py-8 gap-3">
            <CheckCircle className="h-8 w-8 text-green-600" />
            <p className="text-sm font-medium text-green-700">
              Translation completed successfully!
            </p>
            {jobStatus?.characterCount && (
              <p className="text-xs text-muted-foreground">
                {jobStatus.characterCount.toLocaleString()} characters
                translated
              </p>
            )}
          </div>
        )}

        {/* Error state */}
        {(isFailed || requestTranslation.isError) && (
          <div className="flex flex-col items-center justify-center py-8 gap-3">
            <XCircle className="h-8 w-8 text-red-600" />
            <p className="text-sm font-medium text-red-700">
              Translation failed
            </p>
            <p className="text-xs text-muted-foreground text-center">
              {jobStatus?.errorMessage ??
                requestTranslation.error?.message ??
                "An unexpected error occurred. Please try again."}
            </p>
          </div>
        )}

        <DialogFooter>
          {!jobId && !isSubmitting && (
            <>
              <Button variant="outline" onClick={handleClose}>
                Cancel
              </Button>
              <Button
                onClick={handleSubmit}
                disabled={!targetLanguage}
              >
                <Languages className="h-4 w-4 mr-1" />
                Translate
              </Button>
            </>
          )}
          {(isComplete || isFailed) && (
            <Button onClick={handleClose}>Close</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
