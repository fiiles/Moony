import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Compass } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/common/EmptyState';

/** Unknown route: an empty state with the way back, not an error page. */
export default function NotFound() {
  const { t } = useTranslation('common');

  return (
    <div className="flex min-h-[60vh] w-full items-center justify-center p-6">
      <div className="w-full max-w-md">
        <EmptyState
          icon={<Compass aria-hidden="true" />}
          title={t('notFound.title')}
          description={t('notFound.description')}
          action={
            <Button asChild>
              <Link href="/">{t('notFound.backToOverview')}</Link>
            </Button>
          }
        />
      </div>
    </div>
  );
}
