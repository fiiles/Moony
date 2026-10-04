import { useState, useEffect } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
  DialogDescription,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Plus,
  Trash2,
  Pencil,
  ImagePlus,
  X,
  ChevronLeft,
  ChevronRight,
  Upload,
  Download,
} from 'lucide-react';
import { realEstateApi } from '@/lib/tauri-api';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { RealEstatePhotoBatch, RealEstatePhoto } from '@shared/schema';
import { convertFileSrc } from '@tauri-apps/api/core';
import { appDataDir } from '@tauri-apps/api/path';
import { ConfirmDeleteDialog } from '@/components/common/ConfirmDeleteDialog';

interface PhotoTimelineGalleryProps {
  realEstateId: string;
}

// Supported MIME types for direct processing
const SUPPORTED_MIME_TYPES = ['image/jpeg', 'image/png', 'image/gif', 'image/webp', 'image/bmp'];

// Check if file is HEIC (by extension or MIME type)
const isHeicFile = (file: File) => {
  const ext = file.name.toLowerCase().split('.').pop();
  return (
    ext === 'heic' || ext === 'heif' || file.type === 'image/heic' || file.type === 'image/heif'
  );
};

export function PhotoTimelineGallery({ realEstateId }: PhotoTimelineGalleryProps) {
  const { t } = useTranslation('realEstate');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const queryClient = useQueryClient();

  const [isUploadOpen, setIsUploadOpen] = useState(false);
  const [isEditOpen, setIsEditOpen] = useState(false);
  const [editingBatch, setEditingBatch] = useState<RealEstatePhotoBatch | null>(null);
  const [lightboxOpen, setLightboxOpen] = useState(false);
  const [lightboxPhotos, setLightboxPhotos] = useState<RealEstatePhoto[]>([]);
  const [lightboxIndex, setLightboxIndex] = useState(0);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [deletingBatchId, setDeletingBatchId] = useState<string | null>(null);
  const [appDataPath, setAppDataPath] = useState<string>('');
  const [isConverting, setIsConverting] = useState(false);

  // Upload form state
  const [uploadDate, setUploadDate] = useState(new Date().toISOString().split('T')[0]);
  const [uploadDescription, setUploadDescription] = useState('');
  const [selectedFiles, setSelectedFiles] = useState<File[]>([]);

  // Get app data path for resolving file URLs
  useEffect(() => {
    appDataDir().then(setAppDataPath);
  }, []);

  const { data: batches = [], isLoading } = useQuery<RealEstatePhotoBatch[]>({
    queryKey: ['real-estate-photo-batches', realEstateId],
    queryFn: () => realEstateApi.getPhotoBatches(realEstateId),
  });

  // Process files - convert HEIC to JPEG, pass through supported formats
  const processFiles = async (files: File[]): Promise<{ processed: File[]; errors: string[] }> => {
    const processed: File[] = [];
    const errors: string[] = [];

    for (const file of files) {
      // Check if HEIC - convert to JPEG
      if (isHeicFile(file)) {
        try {
          // Dynamic import heic2any
          const heic2any = (await import('heic2any')).default;
          const convertedBlob = await heic2any({
            blob: file,
            toType: 'image/jpeg',
            quality: 0.9,
          });

          // heic2any can return array or single blob
          const blob = Array.isArray(convertedBlob) ? convertedBlob[0] : convertedBlob;
          const newName = file.name.replace(/\.(heic|heif)$/i, '.jpg');
          const convertedFile = new File([blob], newName, { type: 'image/jpeg' });
          processed.push(convertedFile);
        } catch (e) {
          console.error('HEIC conversion failed:', e);
          errors.push(`Failed to convert "${file.name}". Please convert manually.`);
        }
        continue;
      }

      // Check for supported types
      if (SUPPORTED_MIME_TYPES.includes(file.type) || file.type.startsWith('image/')) {
        processed.push(file);
      } else {
        errors.push(`"${file.name}" has unsupported format. Use JPG, PNG, GIF, WebP, or HEIC.`);
      }
    }

    return { processed, errors };
  };

  // Save files to temp directory and get paths
  const saveFilesToTemp = async (files: File[]): Promise<string[]> => {
    const paths: string[] = [];
    const { writeFile, BaseDirectory } = await import('@tauri-apps/plugin-fs');
    const { join, tempDir } = await import('@tauri-apps/api/path');
    const tempDirPath = await tempDir();

    for (const file of files) {
      const arrayBuffer = await file.arrayBuffer();
      const uint8Array = new Uint8Array(arrayBuffer);

      // Generate unique filename with proper extension
      const ext = file.name.split('.').pop()?.toLowerCase() || 'jpg';
      const tempFileName = `photo_${Date.now()}_${Math.random().toString(36).slice(2)}.${ext}`;

      await writeFile(tempFileName, uint8Array, { baseDir: BaseDirectory.Temp });
      const fullPath = await join(tempDirPath, tempFileName);
      paths.push(fullPath);
    }

    return paths;
  };

  const createBatchMutation = useMutation({
    mutationFn: async ({
      date,
      description,
      files,
    }: {
      date: number;
      description?: string;
      files: File[];
    }) => {
      // Process files - converts HEIC to JPEG
      const { processed, errors } = await processFiles(files);

      if (errors.length > 0) {
        throw new Error(errors.join('\n'));
      }

      if (processed.length === 0) {
        throw new Error('No valid files to upload');
      }

      // Save files to temp
      const filePaths = await saveFilesToTemp(processed);

      // Create batch
      const batch = await realEstateApi.createPhotoBatch(realEstateId, {
        photoDate: date,
        description,
      });

      // Add photos
      await realEstateApi.addPhotosToBatch(batch.id, filePaths);
      return batch;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-photo-batches', realEstateId] });
      toast(tc('status.success'));
      setIsUploadOpen(false);
      resetUploadForm();
    },
    onError: (error: Error) => {
      console.error('Upload error:', error);
      toast.error(tc('status.error'), { description: error.message || String(error) });
    },
  });

  const addPhotosMutation = useMutation({
    mutationFn: async ({ batchId, files }: { batchId: string; files: File[] }) => {
      const { processed, errors } = await processFiles(files);

      if (errors.length > 0) {
        throw new Error(errors.join('\n'));
      }

      if (processed.length === 0) {
        throw new Error('No valid files to upload');
      }

      const filePaths = await saveFilesToTemp(processed);
      return realEstateApi.addPhotosToBatch(batchId, filePaths);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-photo-batches', realEstateId] });
      toast(tc('status.success'));
    },
    onError: (error: Error) => {
      toast.error(tc('status.error'), { description: error.message });
    },
  });

  const updateBatchMutation = useMutation({
    mutationFn: ({
      batchId,
      data,
    }: {
      batchId: string;
      data: { photoDate?: number; description?: string };
    }) => realEstateApi.updatePhotoBatch(batchId, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-photo-batches', realEstateId] });
      toast(tc('status.success'));
      setIsEditOpen(false);
      setEditingBatch(null);
    },
  });

  const deleteBatchMutation = useMutation({
    mutationFn: (batchId: string) => realEstateApi.deletePhotoBatch(batchId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-photo-batches', realEstateId] });
      toast(tc('status.success'));
      setDeleteConfirmOpen(false);
      setDeletingBatchId(null);
    },
  });

  const deletePhotoMutation = useMutation({
    mutationFn: (photoId: string) => realEstateApi.deletePhoto(photoId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['real-estate-photo-batches', realEstateId] });
    },
  });

  const resetUploadForm = () => {
    setUploadDate(new Date().toISOString().split('T')[0]);
    setUploadDescription('');
    setSelectedFiles([]);
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0) {
      setIsConverting(true);
      const fileList = Array.from(files);

      try {
        // Process and convert files
        const { processed, errors } = await processFiles(fileList);

        if (errors.length > 0) {
          toast.error(t('gallery.invalidFiles'), { description: errors.join('\n') });
        }

        setSelectedFiles(processed);
      } catch (error) {
        console.error('Error processing files:', error);
      } finally {
        setIsConverting(false);
      }
    }
  };

  const handleDownloadPhoto = async (photo: RealEstatePhoto) => {
    if (!appDataPath) return;
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const { copyFile } = await import('@tauri-apps/plugin-fs');
      const { join } = await import('@tauri-apps/api/path');

      const sourcePath = await join(appDataPath, photo.filePath);

      const filePath = await save({
        defaultPath: `photo_${photo.id.slice(0, 8)}.jpg`,
        filters: [{ name: 'Image', extensions: ['jpg'] }],
      });

      if (filePath) {
        await copyFile(sourcePath, filePath);
        toast(tc('status.success'));
      }
    } catch (error) {
      console.error(error);
      toast.error(tc('status.error'));
    }
  };

  const handleDownloadBatch = async (batch: RealEstatePhotoBatch) => {
    if (!appDataPath) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const { copyFile } = await import('@tauri-apps/plugin-fs');
      const { join } = await import('@tauri-apps/api/path');

      const selected = await open({
        directory: true,
        multiple: false,
        title: t('gallery.selectSaveFolder'),
      });

      if (selected) {
        const targetDir = Array.isArray(selected) ? selected[0] : selected;
        const batchDate = new Date(batch.photoDate * 1000).toISOString().split('T')[0];

        let count = 0;
        for (const photo of batch.photos) {
          const sourcePath = await join(appDataPath, photo.filePath);
          const fileName = `batch_${batchDate}_${photo.id.slice(0, 8)}.jpg`;
          const destPath = await join(targetDir, fileName);
          await copyFile(sourcePath, destPath);
          count++;
        }
        toast(tc('status.success'), { description: `${count} photos saved` });
      }
    } catch (error) {
      console.error(error);
      toast.error(tc('status.error'));
    }
  };

  const handleUpload = () => {
    if (selectedFiles.length === 0) return;
    const date = Math.floor(new Date(uploadDate).getTime() / 1000);
    createBatchMutation.mutate({
      date,
      description: uploadDescription || undefined,
      files: selectedFiles,
    });
  };

  const getPhotoUrl = (photo: RealEstatePhoto) => {
    if (appDataPath) {
      const fullPath = `${appDataPath}/${photo.thumbnailPath}`;
      // Use Tauri's convertFileSrc with 'asset' protocol
      const url = convertFileSrc(fullPath, 'asset');
      return url;
    }
    return '';
  };

  const getFullPhotoUrl = (photo: RealEstatePhoto) => {
    if (appDataPath) {
      const fullPath = `${appDataPath}/${photo.filePath}`;
      return convertFileSrc(fullPath, 'asset');
    }
    return '';
  };

  const openLightbox = (batch: RealEstatePhotoBatch, photoIndex: number) => {
    setLightboxPhotos(batch.photos);
    setLightboxIndex(photoIndex);
    setLightboxOpen(true);
  };

  if (isLoading) {
    return (
      <Card>
        <CardContent className="py-8 text-center text-ink-3">{tc('status.loading')}</CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <div>
          <CardTitle>{t('gallery.title')}</CardTitle>
          <CardDescription>{t('gallery.description')}</CardDescription>
        </div>
        <Dialog
          open={isUploadOpen}
          onOpenChange={(open) => {
            setIsUploadOpen(open);
            if (!open) resetUploadForm();
          }}
        >
          <DialogTrigger asChild>
            <Button>
              <Upload className="mr-2 h-4 w-4" />
              {t('gallery.upload')}
            </Button>
          </DialogTrigger>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>{t('gallery.uploadTitle')}</DialogTitle>
              <DialogDescription>{t('gallery.uploadDescription')}</DialogDescription>
            </DialogHeader>
            <div className="space-y-4 py-4">
              <div className="space-y-2">
                <Label>{t('gallery.photos')}</Label>
                <p className="text-xs text-ink-3 mb-2">{t('gallery.supportedFormats')}</p>
                <label className="border-2 border-dashed rounded-r2 p-6 text-center cursor-pointer hover:border-line-hover transition-colors block">
                  <input
                    type="file"
                    multiple
                    accept="image/jpeg,image/png,image/gif,image/webp,image/bmp,image/heic,image/heif,.heic,.heif"
                    onChange={handleFileSelect}
                    className="hidden"
                  />
                  <ImagePlus className="mx-auto h-8 w-8 text-ink-3 mb-2" />
                  <p className="text-sm text-ink-3">
                    {isConverting
                      ? 'Converting HEIC files...'
                      : selectedFiles.length > 0
                        ? t('gallery.filesSelected', { count: selectedFiles.length })
                        : t('gallery.dropzone')}
                  </p>
                </label>
                {selectedFiles.length > 0 && (
                  <div className="mt-2 grid grid-cols-4 gap-2">
                    {selectedFiles.map((file, idx) => (
                      <div
                        key={idx}
                        className="relative aspect-square rounded overflow-hidden bg-well"
                      >
                        <img
                          src={URL.createObjectURL(file)}
                          alt=""
                          className="w-full h-full object-cover"
                        />
                      </div>
                    ))}
                  </div>
                )}
              </div>
              <div className="space-y-2">
                <Label htmlFor="upload-date">{t('gallery.photoDate')}</Label>
                <Input
                  id="upload-date"
                  type="date"
                  value={uploadDate}
                  onChange={(e) => setUploadDate(e.target.value)}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="upload-desc">{t('gallery.batchDescription')}</Label>
                <Textarea
                  id="upload-desc"
                  placeholder={t('gallery.descriptionPlaceholder')}
                  value={uploadDescription}
                  onChange={(e) => setUploadDescription(e.target.value)}
                />
              </div>
            </div>
            <DialogFooter>
              <Button variant="ghost" onClick={() => setIsUploadOpen(false)}>
                {tc('buttons.cancel')}
              </Button>
              <Button
                onClick={handleUpload}
                disabled={selectedFiles.length === 0 || createBatchMutation.isPending}
              >
                {createBatchMutation.isPending ? tc('status.loading') : t('gallery.upload')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardHeader>
      <CardContent>
        {batches.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-48 text-ink-3 border-2 border-dashed rounded-r1">
            <ImagePlus className="h-8 w-8 mb-2 opacity-50" />
            <p>{t('gallery.noPhotos')}</p>
            <p className="text-xs">{t('gallery.addPhotosHint')}</p>
          </div>
        ) : (
          <div className="space-y-8">
            {batches.map((batch) => (
              <div key={batch.id} className="space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <h3 className="font-semibold text-lg">{fmt.day(batch.photoDate)}</h3>
                    {batch.description && <p className="text-sm text-ink-3">{batch.description}</p>}
                  </div>
                  <div className="flex gap-1">
                    <label>
                      <input
                        type="file"
                        multiple
                        accept="image/jpeg,image/png,image/gif,image/webp,image/bmp,image/heic,image/heif,.heic,.heif"
                        className="hidden"
                        onChange={(e) => {
                          const files = e.target.files;
                          if (files && files.length > 0) {
                            addPhotosMutation.mutate({
                              batchId: batch.id,
                              files: Array.from(files),
                            });
                          }
                        }}
                      />
                      <Button variant="ghost" size="icon" asChild title={t('gallery.addMore')}>
                        <span>
                          <Plus className="h-4 w-4" />
                        </span>
                      </Button>
                    </label>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => handleDownloadBatch(batch)}
                      title={t('gallery.downloadBatch')}
                    >
                      <Download className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => {
                        setEditingBatch(batch);
                        setIsEditOpen(true);
                      }}
                      title={tc('buttons.edit')}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="danger"
                      size="icon"
                      onClick={() => {
                        setDeletingBatchId(batch.id);
                        setDeleteConfirmOpen(true);
                      }}
                      title={tc('buttons.delete')}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
                <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-2">
                  {batch.photos.map((photo, idx) => (
                    <div
                      key={photo.id}
                      className="aspect-square relative rounded-r1 overflow-hidden border bg-well cursor-pointer hover:opacity-90 transition-opacity group"
                      onClick={() => openLightbox(batch, idx)}
                    >
                      <img src={getPhotoUrl(photo)} alt="" className="object-cover w-full h-full" />
                      <button
                        className="absolute top-1 right-1 p-1 bg-black/50 rounded opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={(e) => {
                          e.stopPropagation();
                          deletePhotoMutation.mutate(photo.id);
                        }}
                      >
                        <X className="h-3 w-3 text-white" />
                      </button>
                      <button
                        className="absolute top-1 left-1 p-1 bg-black/50 rounded opacity-0 group-hover:opacity-100 transition-opacity"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDownloadPhoto(photo);
                        }}
                        title={t('gallery.downloadPhoto')}
                      >
                        <Download className="h-3 w-3 text-white" />
                      </button>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>

      {/* Edit Batch Dialog */}
      <Dialog open={isEditOpen} onOpenChange={setIsEditOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('gallery.editBatch')}</DialogTitle>
          </DialogHeader>
          {editingBatch && (
            <div className="space-y-4 py-4">
              <div className="space-y-2">
                <Label>{t('gallery.photoDate')}</Label>
                <Input
                  type="date"
                  defaultValue={new Date(editingBatch.photoDate * 1000).toISOString().split('T')[0]}
                  onChange={(e) => {
                    const date = Math.floor(new Date(e.target.value).getTime() / 1000);
                    setEditingBatch({ ...editingBatch, photoDate: date });
                  }}
                />
              </div>
              <div className="space-y-2">
                <Label>{t('gallery.batchDescription')}</Label>
                <Textarea
                  value={editingBatch.description || ''}
                  onChange={(e) =>
                    setEditingBatch({ ...editingBatch, description: e.target.value })
                  }
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => setIsEditOpen(false)}>
              {tc('buttons.cancel')}
            </Button>
            <Button
              onClick={() => {
                if (editingBatch) {
                  updateBatchMutation.mutate({
                    batchId: editingBatch.id,
                    data: {
                      photoDate: editingBatch.photoDate,
                      description: editingBatch.description || undefined,
                    },
                  });
                }
              }}
              disabled={updateBatchMutation.isPending}
            >
              {tc('buttons.save')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDeleteDialog
        open={deleteConfirmOpen}
        onOpenChange={setDeleteConfirmOpen}
        title={t('gallery.deleteConfirmTitle')}
        description={t('gallery.deleteConfirmDesc')}
        onConfirm={() => deletingBatchId && deleteBatchMutation.mutate(deletingBatchId)}
        isPending={deleteBatchMutation.isPending}
      />

      {/* Lightbox */}
      {lightboxOpen && lightboxPhotos.length > 0 && (
        <div
          className="fixed inset-0 z-50 bg-black/90 flex items-center justify-center"
          onClick={() => setLightboxOpen(false)}
        >
          <button
            className="absolute top-4 right-4 p-2 text-white hover:bg-white/10 rounded"
            onClick={() => setLightboxOpen(false)}
          >
            <X className="h-6 w-6" />
          </button>
          <button
            className="absolute top-4 left-4 p-2 text-white hover:bg-white/10 rounded"
            onClick={(e) => {
              e.stopPropagation();
              handleDownloadPhoto(lightboxPhotos[lightboxIndex]);
            }}
            title={t('gallery.downloadPhoto')}
          >
            <Download className="h-6 w-6" />
          </button>
          {lightboxPhotos.length > 1 && (
            <>
              <button
                className="absolute left-4 p-2 text-white hover:bg-white/10 rounded"
                onClick={(e) => {
                  e.stopPropagation();
                  setLightboxIndex((i) => (i - 1 + lightboxPhotos.length) % lightboxPhotos.length);
                }}
              >
                <ChevronLeft className="h-8 w-8" />
              </button>
              <button
                className="absolute right-4 p-2 text-white hover:bg-white/10 rounded"
                onClick={(e) => {
                  e.stopPropagation();
                  setLightboxIndex((i) => (i + 1) % lightboxPhotos.length);
                }}
              >
                <ChevronRight className="h-8 w-8" />
              </button>
            </>
          )}
          <img
            src={getFullPhotoUrl(lightboxPhotos[lightboxIndex])}
            alt=""
            className="max-h-[90vh] max-w-[90vw] object-contain"
            onClick={(e) => e.stopPropagation()}
          />
          <div className="absolute bottom-4 text-white text-sm">
            {lightboxIndex + 1} / {lightboxPhotos.length}
          </div>
        </div>
      )}
    </Card>
  );
}
