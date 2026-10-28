import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { CategoryManager } from '@/components/categories/CategoryManager';
import { SettingsCard } from '@/components/settings/SettingsCard';

/** Settings → Categorization → Kategorie: the shared category manager in a settings card. */
export function CategoriesCard() {
  const { t } = useTranslation('bank_accounts');
  const [creating, setCreating] = useState(false);
  return (
    <SettingsCard
      title={t('categoryManagement.title')}
      description={t('categoryManagement.description')}
      action={
        <Button size="sm" onClick={() => setCreating(true)}>
          <Plus />
          {t('categoryManagement.add')}
        </Button>
      }
    >
      <CategoryManager
        creating={creating}
        onCreatingChange={setCreating}
        className="max-h-[440px]"
      />
    </SettingsCard>
  );
}
