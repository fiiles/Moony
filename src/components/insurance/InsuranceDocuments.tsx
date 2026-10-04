import { useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { ExternalLink, Plus, X } from 'lucide-react';
import type { InsuranceDocument } from '@shared/schema';
import { insuranceApi } from '@/lib/tauri-api';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';

interface InsuranceDocumentsProps {
  insuranceId: string;
  /** Opens the add-document modal the page owns. */
  onAdd: () => void;
}

const formatFileSize = (bytes: number | null) => {
  if (!bytes) return '';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} kB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const extensionOf = (path: string) => {
  const match = /\.([a-z0-9]{1,4})$/i.exec(path);
  return match ? match[1].toUpperCase() : 'DOC';
};

/**
 * Documents of a policy (prototype insurance-detail.html): a table with the
 * file tile, type badge, upload day and open / delete actions, stored in the
 * app's encrypted folder.
 */
export function InsuranceDocuments({ insuranceId, onAdd }: InsuranceDocumentsProps) {
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();
  const [pendingDelete, setPendingDelete] = useState<InsuranceDocument | null>(null);

  const { data: documents = [], isLoading } = useQuery<InsuranceDocument[]>({
    queryKey: ['insurance-documents', insuranceId],
    queryFn: () => insuranceApi.getDocuments(insuranceId),
  });

  const deleteMutation = useMutation({
    mutationFn: (documentId: string) => insuranceApi.deleteDocument(documentId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['insurance-documents', insuranceId] });
      setPendingDelete(null);
      toast(t('documents.deleted'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: translateApiError(error, tc) });
    },
  });

  const openDocument = async (documentId: string) => {
    try {
      await insuranceApi.openDocument(documentId);
    } catch (error) {
      toast.error(tc('status.error'), { description: String(error) });
    }
  };

  return (
    <>
      <Card variant="table">
        <CardHeader>
          <div>
            <CardTitle>{t('detail.documents')}</CardTitle>
            <CardDescription>{t('documents.subtitle')}</CardDescription>
          </div>
          <Button variant="outline" size="sm" onClick={onAdd}>
            <Plus />
            {t('documents.add')}
          </Button>
        </CardHeader>
        {isLoading ? (
          <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
            {t('loading')}
          </p>
        ) : documents.length === 0 ? (
          <p className="border-t border-line px-[14px] py-8 text-center text-table text-ink-3">
            {t('documents.noDocuments')}
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('documents.columns.document')}</TableHead>
                <TableHead>{t('documents.columns.type')}</TableHead>
                <TableHead className="text-right">{t('documents.columns.uploaded')}</TableHead>
                <TableHead className="w-[72px]">
                  <span className="sr-only">{tc('labels.actions')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {documents.map((doc) => (
                <TableRow key={doc.id} className="h-[50px]">
                  <TableCell>
                    <div className="flex min-w-0 items-center gap-[10px]">
                      <span className="inline-grid size-8 shrink-0 place-items-center rounded-r2 bg-well-2 text-[9px] font-700 tracking-[0.02em] text-ink-3">
                        {extensionOf(doc.filePath)}
                      </span>
                      <div className="min-w-0">
                        <b className="block truncate text-table font-650 text-ink">{doc.name}</b>
                        <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                          {[formatFileSize(doc.fileSize), doc.description]
                            .filter(Boolean)
                            .join(' · ')}
                        </small>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge>
                      {t(`documents.types.${doc.fileType}`, { defaultValue: doc.fileType })}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-right num text-ink-3">
                    {fmt.day(doc.uploadedAt)}
                  </TableCell>
                  <TableCell className="text-right">
                    <div data-row-actions className="inline-flex gap-0.5">
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={t('documents.open')}
                        title={t('documents.open')}
                        onClick={() => openDocument(doc.id)}
                      >
                        <ExternalLink />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={t('documents.delete')}
                        title={t('documents.delete')}
                        onClick={() => setPendingDelete(doc)}
                      >
                        <X />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      <ConfirmDeleteDialog
        open={pendingDelete !== null}
        onOpenChange={(next) => {
          if (!next) setPendingDelete(null);
        }}
        title={t('documents.deleteConfirm.title')}
        description={t('documents.deleteConfirm.description', { name: pendingDelete?.name ?? '' })}
        onConfirm={() => pendingDelete && deleteMutation.mutate(pendingDelete.id)}
        isPending={deleteMutation.isPending}
        confirmLabel={t('documents.delete')}
      />
    </>
  );
}
