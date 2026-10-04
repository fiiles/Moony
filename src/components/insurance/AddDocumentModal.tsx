import { useState } from 'react';
import { useForm } from 'react-hook-form';
import { zodResolver } from '@hookform/resolvers/zod';
import { z } from 'zod';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { insuranceApi } from '@/lib/tauri-api';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Textarea } from '@/components/ui/textarea';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { FileText, Upload } from 'lucide-react';

const documentFormSchema = z.object({
  name: z.string().min(1, 'validation.nameRequired'),
  description: z.string().optional(),
  fileType: z.string().min(1, 'validation.required'),
  filePath: z.string().min(1, 'validation.fileRequired'),
});

type DocumentFormData = z.infer<typeof documentFormSchema>;

interface AddDocumentModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  insuranceId: string;
}

export function AddDocumentModal({
  open: isOpen,
  onOpenChange,
  insuranceId,
}: AddDocumentModalProps) {
  const { t } = useTranslation('insurance');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [selectedFileName, setSelectedFileName] = useState<string | null>(null);

  const form = useForm<DocumentFormData>({
    resolver: zodResolver(documentFormSchema),
    defaultValues: {
      name: '',
      description: '',
      fileType: 'contract',
      filePath: '',
    },
  });

  const createMutation = useMutation({
    mutationFn: async (data: DocumentFormData) => {
      return insuranceApi.addDocument(insuranceId, data.filePath, {
        name: data.name,
        description: data.description || undefined,
        fileType: data.fileType,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['insurance-documents', insuranceId] });
      onOpenChange(false);
      form.reset();
      setSelectedFileName(null);
      toast(tc('status.success'), { description: t('documents.added') });
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const handleSelectFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: 'Documents',
            extensions: ['pdf', 'doc', 'docx', 'jpg', 'jpeg', 'png', 'gif'],
          },
        ],
      });

      if (selected && typeof selected === 'string') {
        form.setValue('filePath', selected);

        // Extract filename for display and auto-fill name if empty
        const fileName = selected.split('/').pop() || selected.split('\\').pop() || selected;
        setSelectedFileName(fileName);

        if (!form.getValues('name')) {
          // Remove extension for auto-fill
          const nameWithoutExt = fileName.replace(/\.[^/.]+$/, '');
          form.setValue('name', nameWithoutExt);
        }
      }
    } catch (error) {
      toast.error(tc('status.error'), { description: String(error) });
    }
  };

  const onSubmit = (data: DocumentFormData) => {
    createMutation.mutate(data);
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) {
          form.reset();
          setSelectedFileName(null);
        }
        onOpenChange(open);
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('documents.addTitle')}</DialogTitle>
        </DialogHeader>

        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className="space-y-4">
            {/* File Selection */}
            <FormField
              control={form.control}
              name="filePath"
              render={({ field: _field }) => (
                <FormItem>
                  <FormLabel>{t('documents.file')}</FormLabel>
                  <FormControl>
                    <div
                      className="border-2 border-dashed rounded-r2 p-6 text-center cursor-pointer hover:border-line-hover transition-colors overflow-hidden"
                      onClick={handleSelectFile}
                    >
                      {selectedFileName ? (
                        (() => {
                          // Split filename to preserve extension
                          const lastDot = selectedFileName.lastIndexOf('.');
                          const name =
                            lastDot > 0 ? selectedFileName.slice(0, lastDot) : selectedFileName;
                          const ext = lastDot > 0 ? selectedFileName.slice(lastDot) : '';
                          const maxNameLength = 25;
                          const displayName =
                            name.length > maxNameLength
                              ? name.slice(0, maxNameLength) + '...' + ext
                              : selectedFileName;
                          return (
                            <div className="flex items-center justify-center gap-2 min-w-0 w-full">
                              <FileText className="h-5 w-5 flex-shrink-0 text-ink" />
                              <span className="font-medium" title={selectedFileName}>
                                {displayName}
                              </span>
                            </div>
                          );
                        })()
                      ) : (
                        <div className="space-y-2">
                          <Upload className="mx-auto h-8 w-8 text-ink-3" />
                          <p className="text-sm text-ink-3">{t('documents.clickToSelect')}</p>
                        </div>
                      )}
                    </div>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            {/* Document Name */}
            <FormField
              control={form.control}
              name="name"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{tc('labels.name')}</FormLabel>
                  <FormControl>
                    <Input {...field} placeholder={t('documents.namePlaceholder')} />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            {/* Document Type */}
            <FormField
              control={form.control}
              name="fileType"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>{t('documents.type')}</FormLabel>
                  <Select onValueChange={field.onChange} defaultValue={field.value}>
                    <FormControl>
                      <SelectTrigger>
                        <SelectValue placeholder={t('documents.selectType')} />
                      </SelectTrigger>
                    </FormControl>
                    <SelectContent>
                      <SelectItem value="contract">{t('documents.types.contract')}</SelectItem>
                      <SelectItem value="certificate">
                        {t('documents.types.certificate')}
                      </SelectItem>
                      <SelectItem value="claim">{t('documents.types.claim')}</SelectItem>
                      <SelectItem value="other">{t('documents.types.other')}</SelectItem>
                    </SelectContent>
                  </Select>
                  <FormMessage />
                </FormItem>
              )}
            />

            {/* Description */}
            <FormField
              control={form.control}
              name="description"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>
                    {tc('labels.description')} ({t('documents.optional')})
                  </FormLabel>
                  <FormControl>
                    <Textarea
                      {...field}
                      value={field.value || ''}
                      placeholder={t('documents.descriptionPlaceholder')}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            <div className="flex justify-end gap-2 pt-4">
              <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
                {tc('buttons.cancel')}
              </Button>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? t('documents.uploading') : t('documents.upload')}
              </Button>
            </div>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}
