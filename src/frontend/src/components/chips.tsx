import {
  FileText,
  FileSpreadsheet,
  Presentation,
  FileType2,
  Image as ImageIcon,
  Folder,
  Mail,
  File,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import {
  parseItemType,
  parseDocumentFormat,
  DOCUMENT_FORMAT_LABEL,
  type ItemType,
} from "@/lib/enums";
import type { Wire } from "@/types";

const ITEM_META: Record<ItemType, { icon: typeof FileText; label: string }> = {
  Email: { icon: Mail, label: "Email" },
  Document: { icon: FileText, label: "Document" },
  Spreadsheet: { icon: FileSpreadsheet, label: "Spreadsheet" },
  Presentation: { icon: Presentation, label: "Presentation" },
  Pdf: { icon: FileType2, label: "PDF" },
  Image: { icon: ImageIcon, label: "Image" },
  Folder: { icon: Folder, label: "Folder" },
  Other: { icon: File, label: "Other" },
};

export function ItemTypeChip({ itemType }: { itemType: Wire }) {
  const key = parseItemType(itemType);
  const { icon: Icon, label } = ITEM_META[key];
  return (
    <Badge variant="secondary" className="font-normal">
      <Icon className="h-3 w-3" />
      {label}
    </Badge>
  );
}

export function DocumentFormatChip({ format }: { format: Wire }) {
  const key = parseDocumentFormat(format);
  return (
    <Badge variant="secondary" className="font-normal">
      {DOCUMENT_FORMAT_LABEL[key]}
    </Badge>
  );
}
