import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import * as opener from '@tauri-apps/plugin-opener';
import { getVersion } from '@tauri-apps/api/app';
import { toast } from 'sonner';
import { BookOpen, Bug, ExternalLink, FileText, FolderOpen, Heart } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { UpdateStatusLine } from '@/components/common/UpdateStatusLine';
import { useUpdater } from '@/hooks/use-updater';
import { DOCS_URL, LICENSE_URL, REPO_URL, REPORT_BUG_URL } from '@/lib/app-links';
import { dataApi, systemApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

// lucide-react v1 removed brand icons; inline GitHub mark instead
function GithubIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
      <path d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12" />
    </svg>
  );
}

interface AboutModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function AboutModal({ open: isOpen, onOpenChange: setIsOpen }: AboutModalProps) {
  const { t } = useTranslation('common');
  const [version, setVersion] = useState<string>('');
  const { status, check, install } = useUpdater();

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(''));
  }, []);

  // Same key as Settings → Data and backups, so the path is shared; fetched only while open.
  const { data: location } = useQuery({
    queryKey: ['data-location'],
    queryFn: () => dataApi.getDataLocation(),
    enabled: isOpen,
  });

  const showError = (error: unknown) => {
    const message =
      error instanceof Error ? translateApiError(error, t) : translateApiError(String(error), t);
    toast.error(t('status.error'), { description: message });
  };

  const handleOpenLink = async (url: string) => {
    try {
      await opener.openUrl(url);
    } catch (error) {
      // Fallback to window.open if Tauri opener fails
      console.error('Failed to open link, using fallback:', error);
      window.open(url, '_blank');
    }
  };

  const handleOpenDataFolder = async () => {
    try {
      await dataApi.openDataFolder();
    } catch (error) {
      showError(error);
    }
  };

  const handleOpenLogs = async () => {
    try {
      await systemApi.openLogsFolder();
    } catch (error) {
      showError(error);
    }
  };

  // The shared update dialog shows release notes and progress; close this modal so only one is open.
  const handleInstall = () => {
    setIsOpen(false);
    void install();
  };

  const busy = status === 'checking' || status === 'installing';

  return (
    <Dialog open={isOpen} onOpenChange={setIsOpen}>
      <DialogContent className="max-h-[calc(100vh-2rem)] gap-3 overflow-y-auto p-5 sm:max-w-lg">
        <DialogHeader className="pb-1">
          <div className="flex items-center gap-3">
            <div className="flex aspect-square size-12 shrink-0 items-center justify-center rounded-r2 overflow-hidden">
              <img src="/moony-icon.png" alt="Moony" className="size-12" />
            </div>
            <div className="grid flex-1 text-left leading-tight">
              <DialogTitle className="text-xl font-bold">{t('app.name')}</DialogTitle>
              <DialogDescription className="text-sm">{t('app.tagline')}</DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="space-y-2">
          {/* Version and update status */}
          <div className="rounded-r2 border bg-well p-3 space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-sm text-ink-3">{t('about.version')}</span>
              <span className="text-sm font-semibold">{version}</span>
            </div>
            <div className="flex items-center justify-between gap-3 border-t pt-3">
              <UpdateStatusLine />
              {status === 'available' ? (
                <Button size="sm" className="shrink-0" onClick={handleInstall}>
                  {t('update.install')}
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="outline"
                  className="shrink-0"
                  onClick={() => void check()}
                  disabled={busy}
                >
                  {t('update.checkNow')}
                </Button>
              )}
            </div>
          </div>

          {/* Description */}
          <p className="px-1 text-sm leading-normal text-ink-3">{t('about.description')}</p>

          {/* Data folder */}
          <div className="rounded-r2 border bg-well p-3 space-y-2">
            <div className="text-sm font-medium">{t('about.dataFolder')}</div>
            <div className="flex items-center gap-3">
              <code
                className="min-w-0 flex-1 break-all text-xs text-ink-3"
                title={location?.dataDir}
              >
                {location?.dataDir ?? '…'}
              </code>
              <Button
                size="sm"
                variant="outline"
                className="shrink-0"
                onClick={handleOpenDataFolder}
              >
                <FolderOpen className="mr-2 h-4 w-4" />
                {t('about.openFolder')}
              </Button>
            </div>
          </div>

          {/* Links */}
          <div className="grid grid-cols-2 gap-2">
            <Button
              variant="outline"
              className="justify-start gap-2"
              onClick={() => handleOpenLink(REPO_URL)}
            >
              <GithubIcon className="w-4 h-4 shrink-0" />
              <span className="truncate">{t('about.viewOnGithub')}</span>
              <ExternalLink className="w-3 h-3 ml-auto shrink-0 opacity-50" />
            </Button>
            <Button
              variant="outline"
              className="justify-start gap-2"
              onClick={() => handleOpenLink(DOCS_URL)}
            >
              <BookOpen className="w-4 h-4 shrink-0" />
              <span className="truncate">{t('about.documentation')}</span>
              <ExternalLink className="w-3 h-3 ml-auto shrink-0 opacity-50" />
            </Button>
            <Button
              variant="outline"
              className="justify-start gap-2"
              onClick={() => handleOpenLink(REPORT_BUG_URL)}
            >
              <Bug className="w-4 h-4 shrink-0" />
              <span className="truncate">{t('about.reportBug')}</span>
              <ExternalLink className="w-3 h-3 ml-auto shrink-0 opacity-50" />
            </Button>
            <Button variant="outline" className="justify-start gap-2" onClick={handleOpenLogs}>
              <FileText className="w-4 h-4 shrink-0" />
              <span className="truncate">{t('about.openLogs')}</span>
            </Button>
          </div>

          {/* License notice — AGPL-3.0 §5(d) Appropriate Legal Notices */}
          <div className="rounded-r2 border bg-well p-3 space-y-1.5">
            <p className="text-xs text-ink-3">{t('about.copyright')}</p>
            <p className="text-xs text-ink-3 leading-relaxed">{t('about.licenseNotice')}</p>
            <button
              onClick={() => handleOpenLink(LICENSE_URL)}
              className="inline-flex items-center gap-1 text-xs font-medium text-ink hover:underline"
            >
              {t('about.viewLicense')}
              <ExternalLink className="w-3 h-3 opacity-50" />
            </button>
          </div>

          {/* Footer */}
          <div className="flex items-center justify-center gap-1 text-xs text-ink-3">
            <span>{t('about.madeWith')}</span>
            <Heart className="w-3 h-3 text-red-500 fill-red-500 animate-pulse" />
            <span>{t('about.byAuthor')}</span>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
