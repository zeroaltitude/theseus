// Formatting for dense readouts: short, aligned, and never lying about precision.
import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export * from './figures.ts'

export const cn = (...v: ClassValue[]) => twMerge(clsx(v))
