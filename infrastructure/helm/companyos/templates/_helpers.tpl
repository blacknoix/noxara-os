{{/*
CompanyOS Helm helpers
*/}}
{{- define "companyos.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "companyos.fullname" -}}
{{- default .Release.Name .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "companyos.labels" -}}
app.kubernetes.io/name: {{ include "companyos.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: companyos
companyos.io/cell-id: {{ .Values.cell.id | quote }}
companyos.io/cell-region: {{ .Values.cell.region | quote }}
{{- end -}}

{{- define "companyos.image" -}}
{{- $svc := .svc -}}
{{- $root := .root -}}
{{- if $root.Values.image.registry -}}
{{ printf "%s/%s:%s" $root.Values.image.registry $svc.image $root.Values.image.tag }}
{{- else -}}
{{ printf "%s:%s" $svc.image $root.Values.image.tag }}
{{- end -}}
{{- end -}}
