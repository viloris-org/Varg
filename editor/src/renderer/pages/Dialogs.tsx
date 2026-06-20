import React, { useCallback } from 'react';
import { useTranslation } from '../i18n';
import { IconAlertTriangle } from '../icons';
import {
  buttonClass,
  modalBodyClass,
  modalClass,
  modalFooterClass,
  modalHeaderClass,
  modalOverlayClass,
  modalTitleClass,
} from '../uiClasses';

// ─── Close Project Dialog (Save/Discard/Cancel) ────────────────────────────

interface CloseProjectDialogProps {
  projectName: string;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

export function CloseProjectDialog({ projectName, onSave, onDiscard, onCancel }: CloseProjectDialogProps) {
  const { t } = useTranslation();

  const handleOverlayClick = useCallback((e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onCancel();
  }, [onCancel]);

  return (
    <div className={modalOverlayClass} onClick={handleOverlayClick}>
      <div className={modalClass('w-[420px]')}>
        <div className={modalHeaderClass}>
          <h3 className={modalTitleClass}>{t('dialog_unsaved_title')}</h3>
        </div>
        <div className={modalBodyClass}>
          <div className="delete-warning">
            <IconAlertTriangle />
            <div className="delete-warning-text">
              {t('dialog_unsaved_message').replace('{project}', projectName)}
            </div>
          </div>
        </div>
        <div className={modalFooterClass}>
          <button className={buttonClass('secondary')} onClick={onCancel}>
            {t('dialog_cancel')}
          </button>
          <button className={buttonClass('danger')} onClick={onDiscard}>
            {t('dialog_discard')}
          </button>
          <button className={buttonClass('primary')} onClick={onSave}>
            {t('dialog_save')}
          </button>
        </div>
      </div>
    </div>
  );
}
