'use client';

import { useRef, useState, type ChangeEvent, type DragEvent, type ReactNode } from 'react';

export type FileUploadProps = {
  label?: ReactNode;
  hint?: ReactNode;
  accept?: string;
  multiple?: boolean;
  disabled?: boolean;
  onFiles?: (files: FileList | null) => void;
};

export function FileUpload({ label, hint, accept, multiple, disabled, onFiles }: FileUploadProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);

  const handleFiles = (files: FileList | null) => {
    onFiles?.(files);
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    if (disabled) return;
    handleFiles(e.dataTransfer.files);
  };

  const onChange = (e: ChangeEvent<HTMLInputElement>) => {
    handleFiles(e.target.files);
  };

  return (
    <div style={{ fontFamily: 'var(--cos-font-sans)', width: '100%' }}>
      {label ? (
        <div style={{ fontSize: '0.8125rem', fontWeight: 600, color: 'var(--cos-color-fg)', marginBottom: 6 }}>
          {label}
        </div>
      ) : null}
      <div
        role="button"
        tabIndex={disabled ? -1 : 0}
        aria-disabled={disabled || undefined}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            inputRef.current?.click();
          }
        }}
        onClick={() => !disabled && inputRef.current?.click()}
        onDragOver={(e) => {
          e.preventDefault();
          if (!disabled) setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
        style={{
          border: `2px dashed ${dragging ? 'var(--cos-color-accent)' : 'var(--cos-color-border)'}`,
          borderRadius: 'var(--cos-radius-md)',
          background: dragging ? 'var(--cos-color-accent-muted)' : 'var(--cos-color-bg-elevated)',
          padding: 'var(--cos-space-8) var(--cos-space-4)',
          textAlign: 'center',
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.55 : 1,
          color: 'var(--cos-color-fg-muted)',
          transition: `border-color var(--cos-duration-fast), background var(--cos-duration-fast)`,
        }}
      >
        <p style={{ margin: 0, color: 'var(--cos-color-fg)', fontWeight: 600 }}>Drop files here</p>
        <p style={{ margin: '0.35rem 0 0', fontSize: '0.8125rem' }}>or click to browse</p>
        <input
          ref={inputRef}
          type="file"
          accept={accept}
          multiple={multiple}
          disabled={disabled}
          onChange={onChange}
          style={{ display: 'none' }}
        />
      </div>
      {hint ? (
        <p style={{ margin: '0.35rem 0 0', fontSize: '0.75rem', color: 'var(--cos-color-fg-muted)' }}>{hint}</p>
      ) : null}
    </div>
  );
}
