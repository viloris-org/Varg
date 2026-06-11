import React, { useCallback } from 'react';
import { useTranslation } from '../i18n';
import { IconAlertTriangle } from '../icons';

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
    <div className="modal-overlay" onClick={handleOverlayClick}>
      <div className="modal" style={{ width: 420 }}>
        <div className="modal-header">
          <h3>{t('dialog_unsaved_title')}</h3>
        </div>
        <div className="modal-body">
          <div className="delete-warning">
            <IconAlertTriangle />
            <div className="delete-warning-text">
              {t('dialog_unsaved_message').replace('{project}', projectName)}
            </div>
          </div>
        </div>
        <div className="modal-footer" style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
          <button className="btn btn-secondary" onClick={onCancel}>
            {t('dialog_cancel')}
          </button>
          <button className="btn btn-danger" onClick={onDiscard}>
            {t('dialog_discard')}
          </button>
          <button className="btn btn-primary" onClick={onSave}>
            {t('dialog_save')}
          </button>
        </div>
      </div>
    </div>
  );
}
